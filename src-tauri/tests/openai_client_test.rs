use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

use build_douyin_acount_lib::analysis::openai::OpenAiClient;

/// 起一次性 HTTP mock server：接收一个请求，把「请求头 + \n\n + body」经 channel 发还测试侧，
/// 然后回复固定响应。返回 (base_url, receiver)。
fn serve_once(status: &str, resp_body: &str) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = format!("http://{}", listener.local_addr().expect("addr"));
    let (tx, rx) = mpsc::channel();
    let status = status.to_string();
    let resp_body = resp_body.to_string();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .ok();
        let mut buf: Vec<u8> = Vec::new();
        let mut tmp = [0u8; 4096];
        // 先读到 header 结束标记。
        let head_end = loop {
            let n = stream.read(&mut tmp).expect("read");
            if n == 0 {
                panic!("connection closed before headers complete");
            }
            buf.extend_from_slice(&tmp[..n]);
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos;
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
        let content_length: usize = head
            .lines()
            .find_map(|l| {
                let lower = l.to_ascii_lowercase();
                lower
                    .strip_prefix("content-length:")
                    .and_then(|v| v.trim().parse().ok())
            })
            .unwrap_or(0);
        // 再按 Content-Length 读满 body。
        let mut body = buf[head_end + 4..].to_vec();
        while body.len() < content_length {
            let n = stream.read(&mut tmp).expect("read body");
            if n == 0 {
                break;
            }
            body.extend_from_slice(&tmp[..n]);
        }
        body.truncate(content_length);
        tx.send(format!("{head}\n\n{}", String::from_utf8_lossy(&body)))
            .expect("send");
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp_body}",
            resp_body.len()
        );
        stream.write_all(resp.as_bytes()).expect("write");
        stream.flush().ok();
    });
    (addr, rx)
}

fn recv_request(rx: &mpsc::Receiver<String>) -> String {
    rx.recv_timeout(std::time::Duration::from_secs(5))
        .expect("request captured")
}

fn body_json(raw: &str) -> serde_json::Value {
    let body = raw.split("\n\n").nth(1).expect("body present");
    serde_json::from_str(body).expect("json body")
}

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Runtime::new()
        .expect("rt")
        .block_on(f)
}

#[test]
fn openai_client_rejects_empty_api_key() {
    let err = match OpenAiClient::new("https://api.kimi.com/coding/v1", "") {
        Ok(_) => panic!("empty key must fail"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("API Key"), "got: {err}");
}

#[test]
fn openai_client_rejects_invalid_base_url() {
    if OpenAiClient::new("not a url", "sk-x").is_ok() {
        panic!("invalid url must fail");
    }
}

#[test]
fn chat_posts_openai_shape_and_parses_content() {
    let (base, rx) = serve_once(
        "200 OK",
        r#"{"choices":[{"message":{"content":"{\"ok\":true}"}}]}"#,
    );
    let client = OpenAiClient::new(&base, "sk-test").expect("client");
    let out = block_on(client.chat("k3", "sys", "usr", true)).expect("chat");
    assert_eq!(out, r#"{"ok":true}"#);

    let raw = recv_request(&rx);
    assert!(
        raw.starts_with("POST /chat/completions HTTP/1.1"),
        "request line: {}",
        raw.lines().next().unwrap_or("")
    );
    assert!(
        raw.to_ascii_lowercase().contains("authorization: bearer sk-test"),
        "auth header missing: {raw}"
    );
    let body = body_json(&raw);
    assert_eq!(body["model"], "k3");
    assert_eq!(
        body["messages"][0],
        serde_json::json!({"role": "system", "content": "sys"})
    );
    assert_eq!(
        body["messages"][1],
        serde_json::json!({"role": "user", "content": "usr"})
    );
    assert_eq!(
        body["response_format"],
        serde_json::json!({"type": "json_object"}),
        "json_mode=true must set response_format"
    );
}

#[test]
fn chat_with_image_sends_base64_data_url() {
    let dir = tempfile::tempdir().expect("tempdir");
    let img = dir.path().join("frame_001.jpg");
    std::fs::write(&img, b"fakejpeg").expect("write img");
    let (base, rx) = serve_once("200 OK", r#"{"choices":[{"message":{"content":"{}"}}]}"#);
    let client = OpenAiClient::new(&base, "sk-test").expect("client");
    block_on(client.chat_with_image("k3", "describe it", &img, false)).expect("chat");

    let raw = recv_request(&rx);
    let body = body_json(&raw);
    let content = &body["messages"][0]["content"];
    assert_eq!(
        content[0],
        serde_json::json!({"type": "text", "text": "describe it"})
    );
    let url = content[1]["image_url"]["url"]
        .as_str()
        .expect("image_url.url");
    assert!(
        url.starts_with("data:image/jpeg;base64,"),
        "data url prefix: {url}"
    );
    assert!(url.ends_with("ZmFrZWpwZWc="), "base64 of b\"fakejpeg\": {url}");
    assert!(
        body.get("response_format").is_none(),
        "json_mode=false must not set response_format"
    );
}

#[test]
fn non_2xx_maps_to_http_error() {
    let (base, _rx) = serve_once("401 Unauthorized", r#"{"error":{"message":"bad key"}}"#);
    let client = OpenAiClient::new(&base, "sk-bad").expect("client");
    let err = block_on(client.chat("k3", "s", "u", false)).unwrap_err();
    assert!(err.to_string().contains("401"), "got: {err}");
}
