use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

use build_douyin_acount_lib::commands::cloud_models::fetch_cloud_models;

/// 起一次性 HTTP mock server：接收一个请求，把请求头经 channel 发还测试侧，然后回复固定响应。
/// 与 openai_client_test.rs 同款桩（各测试文件自成一体）。
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
        let mut head = Vec::new();
        let mut tmp = [0u8; 4096];
        loop {
            let n = stream.read(&mut tmp).expect("read");
            if n == 0 {
                panic!("connection closed before headers complete");
            }
            head.extend_from_slice(&tmp[..n]);
            if head.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        tx.send(String::from_utf8_lossy(&head).to_string()).expect("send");
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

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Runtime::new()
        .expect("rt")
        .block_on(f)
}

#[test]
fn returns_sorted_model_ids_and_skips_entries_without_id() {
    let (base, _rx) = serve_once(
        "200 OK",
        r#"{"data":[{"id":"b-model"},{"id":"a-model"},{"object":"model"}]}"#,
    );
    let ids = block_on(fetch_cloud_models(&base, "sk-test")).expect("ids");
    assert_eq!(ids, vec!["a-model", "b-model"]);
}

#[test]
fn sends_bearer_header_when_key_present() {
    let (base, rx) = serve_once("200 OK", r#"{"data":[]}"#);
    block_on(fetch_cloud_models(&base, "sk-secret")).expect("ok");
    let req = recv_request(&rx);
    assert!(
        req.to_ascii_lowercase().contains("authorization: bearer sk-secret"),
        "request should carry bearer header, got: {req}"
    );
}

#[test]
fn omits_bearer_header_when_key_empty() {
    let (base, rx) = serve_once("200 OK", r#"{"data":[]}"#);
    block_on(fetch_cloud_models(&base, "  ")).expect("ok");
    let req = recv_request(&rx);
    assert!(
        !req.to_ascii_lowercase().contains("authorization:"),
        "empty key must not send authorization header, got: {req}"
    );
}

#[test]
fn http_error_status_propagates() {
    let (base, _rx) = serve_once("401 Unauthorized", r#"{"error":"invalid key"}"#);
    let err = block_on(fetch_cloud_models(&base, "bad-key")).expect_err("should fail");
    assert!(matches!(err, build_douyin_acount_lib::domain::error::DomainError::Http { status: 401, .. }));
}

#[test]
fn missing_data_array_is_invalid_data() {
    let (base, _rx) = serve_once("200 OK", r#"{"object":"list"}"#);
    let err = block_on(fetch_cloud_models(&base, "k")).expect_err("should fail");
    assert!(matches!(
        err,
        build_douyin_acount_lib::domain::error::DomainError::InvalidData(_)
    ));
}
