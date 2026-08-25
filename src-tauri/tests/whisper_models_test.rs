//! Whisper 模型下载器集成测试：本地迷你 HTTP server 验证
//! 检测判据 / 完整下载 / 官方失败自动切镜像 / 断点续传 / 取消保留断点。
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use build_douyin_acount_lib::whisper_models::{
    Hosts, RoutePref, WhisperModelSpec, download_model, find_spec, is_installed,
};

/// 测试用小模型（model.bin 需 >1MB 才过安装判据）。
static TEST_SPEC: WhisperModelSpec = WhisperModelSpec {
    name: "test",
    size_mb: 3,
    vram_hint: "",
    files: &["config.json", "model.bin"],
};

#[derive(Clone, Copy)]
enum Mode {
    /// 正常服务（支持 HEAD 与 Range）。
    Full,
    /// 一律 500（模拟官方源被墙/故障）。
    Fail500,
    /// 慢速滴灌（用于取消测试：小块 + 间隔）。
    SlowDrip,
}

struct MockServer {
    url: String,
    /// 记录收到的 Range 头（如 "bytes=100-"），验证断点续传。
    ranges: Arc<Mutex<Vec<String>>>,
}

fn start_server(body: Vec<u8>, mode: Mode) -> MockServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let ranges2 = ranges.clone();
    // accept 循环随测试进程退出而消亡，无需停止机制。
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(s) = stream else { continue };
            let body = body.clone();
            let ranges = ranges2.clone();
            std::thread::spawn(move || handle_conn(s, &body, mode, &ranges));
        }
    });
    MockServer {
        url: format!("http://{addr}"),
        ranges,
    }
}

fn handle_conn(mut s: TcpStream, body: &[u8], mode: Mode, ranges: &Mutex<Vec<String>>) {
    // 读请求头直到空行。
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        match s.read(&mut tmp) {
            Ok(0) => return,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => return,
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let first = text.lines().next().unwrap_or("");
    let is_head = first.starts_with("HEAD");
    let range = text
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("range:"))
        .map(|l| l[6..].trim().to_string());
    if let Some(r) = &range {
        ranges.lock().expect("ranges").push(r.clone());
    }

    if matches!(mode, Mode::Fail500) {
        let _ = s.write_all(
            b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        return;
    }

    if is_head {
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = s.write_all(resp.as_bytes());
        return;
    }

    let start: usize = range
        .as_deref()
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.split('-').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let chunk_body = &body[start.min(body.len())..];
    let (status, content_range) = if start > 0 {
        (
            "206 Partial Content",
            format!("Content-Range: bytes {}-{}/{}\r\n", start, body.len() - 1, body.len()),
        )
    } else {
        ("200 OK", String::new())
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{content_range}Connection: close\r\n\r\n",
        chunk_body.len()
    );
    if s.write_all(head.as_bytes()).is_err() {
        return;
    }
    if matches!(mode, Mode::SlowDrip) {
        for chunk in chunk_body.chunks(4096) {
            if s.write_all(chunk).is_err() {
                return;
            }
            let _ = s.flush();
            std::thread::sleep(Duration::from_millis(20));
        }
    } else {
        let _ = s.write_all(chunk_body);
    }
}

fn hosts(official: &str, mirror: &str) -> Hosts {
    Hosts {
        official: official.to_string(),
        mirror: mirror.to_string(),
    }
}

fn read_progress(path: &Path) -> serde_json::Value {
    let raw = std::fs::read_to_string(path).expect("progress file");
    serde_json::from_str(&raw).expect("progress json")
}

#[test]
fn detect_requires_all_files_and_nonempty_model_bin() {
    let spec = find_spec("tiny").expect("tiny spec");
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    assert!(!is_installed(spec, root), "空目录不应判定已安装");
    for f in spec.files {
        std::fs::write(root.join(f), b"x").expect("write");
    }
    assert!(!is_installed(spec, root), "model.bin 过小不应判定已安装");
    std::fs::write(root.join("model.bin"), vec![7u8; 2_000_000]).expect("write model.bin");
    assert!(is_installed(spec, root));
}

#[test]
fn download_completes_and_renames_part() {
    let server = start_server(vec![7u8; 2_000_000], Mode::Full);
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("model");
    std::fs::create_dir_all(&dest).expect("mkdir");
    let progress = dir.path().join("progress.json");

    download_model(
        &TEST_SPEC,
        &dest,
        RoutePref::Official,
        &hosts(&server.url, "http://127.0.0.1:1"),
        &progress,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("download");

    assert_eq!(
        std::fs::read(dest.join("model.bin")).expect("model.bin"),
        vec![7u8; 2_000_000]
    );
    assert!(!dest.join("model.bin.part").exists(), "完成后 .part 应已改名");
    assert!(is_installed(&TEST_SPEC, &dest));
    let p = read_progress(&progress);
    assert_eq!(p["stage"], "done");
    // 两个文件 HEAD 都成功 → total_bytes 为实际大小之和。
    assert_eq!(p["total_bytes"], 4_000_000u64);
}

#[test]
fn download_falls_back_to_mirror_on_failure() {
    let official = start_server(vec![], Mode::Fail500);
    let mirror = start_server(vec![9u8; 2_000_000], Mode::Full);
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("model");
    std::fs::create_dir_all(&dest).expect("mkdir");
    let progress = dir.path().join("progress.json");

    download_model(
        &TEST_SPEC,
        &dest,
        RoutePref::Auto,
        &hosts(&official.url, &mirror.url),
        &progress,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("download via mirror");

    assert!(is_installed(&TEST_SPEC, &dest));
    let p = read_progress(&progress);
    assert_eq!(p["stage"], "done");
    assert_eq!(p["route"], "mirror", "官方 500 后应自动切换镜像");
}

#[test]
fn download_resumes_partial_file() {
    let body = vec![5u8; 2_000_000];
    let server = start_server(body.clone(), Mode::Full);
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("model");
    std::fs::create_dir_all(&dest).expect("mkdir");
    // 预置半截 .part（前 100 字节）。
    std::fs::write(dest.join("model.bin.part"), &body[..100]).expect("seed part");
    let progress = dir.path().join("progress.json");

    download_model(
        &TEST_SPEC,
        &dest,
        RoutePref::Mirror,
        &hosts("http://127.0.0.1:1", &server.url),
        &progress,
        Arc::new(AtomicBool::new(false)),
    )
    .expect("download");

    assert_eq!(
        std::fs::read(dest.join("model.bin")).expect("model.bin"),
        body
    );
    let ranges = server.ranges.lock().expect("ranges");
    assert_eq!(ranges.as_slice(), ["bytes=100-"], "应带 Range 头续传");
}

#[test]
fn cancel_keeps_part_file() {
    let server = start_server(vec![3u8; 20_000_000], Mode::SlowDrip);
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("model");
    std::fs::create_dir_all(&dest).expect("mkdir");
    let progress = dir.path().join("progress.json");
    let cancel = Arc::new(AtomicBool::new(false));

    let cancel2 = cancel.clone();
    let dest2 = dest.clone();
    let progress2 = progress.clone();
    let url = server.url.clone();
    let handle = std::thread::spawn(move || {
        let _ = download_model(
            &TEST_SPEC,
            &dest2,
            RoutePref::Official,
            &hosts(&url, "http://127.0.0.1:1"),
            &progress2,
            cancel2,
        );
    });
    // 等断点文件真正写入数据后再取消（固定 sleep 在全量测试并发下时序不稳定）。
    let part = dest.join("config.json.part");
    let mut waited = 0;
    while std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) == 0 && waited < 10_000 {
        std::thread::sleep(Duration::from_millis(50));
        waited += 50;
    }
    assert!(waited < 10_000, "10s 内断点文件应有数据写入");
    cancel.store(true, Ordering::Relaxed);
    handle.join().expect("join");

    let p = read_progress(&progress);
    assert_eq!(p["stage"], "cancelled");
    let part_len = std::fs::metadata(&part).expect("part kept").len();
    assert!(part_len > 0 && part_len < 20_000_000, "断点应保留部分数据");
    assert!(!dest.join("config.json").exists(), "未完成文件不应改名");
}
