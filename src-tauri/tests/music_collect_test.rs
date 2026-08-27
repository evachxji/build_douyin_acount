//! 音乐库采集集成测试：本地 mock server 守护 API 解析（19 位 id/翻页补足/去重）、
//! 下载落盘命名、分类映射、幂等重跑、取消停止、单曲失败跳过不中断。
use std::collections::{HashMap, HashSet};
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::media::MediaStore;
use build_douyin_acount_lib::music_collect::{CollectProgress, run_collect};

const CAT1: u64 = 7405872200315882277;
const CAT2: u64 = 6685253914809338638;
/// 19 位歌曲 id（超 i64::MAX）：映射表必须存 TEXT 且不丢精度。
const S1: u64 = 7377935002954319891;
const S2: u64 = 7377935002954319892;
const S3: u64 = 7377935002954319893;
const S4: u64 = 7377935002954319894;

struct Mock {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

/// pages: (collection_id, cursor) → (songs JSON 数组, has_more, next_offset)。
/// songs 里的 preview_url 用 "@BASE@/a/<id>" 占位，响应时替换为真实地址。
#[allow(clippy::too_many_arguments)]
fn start_mock(
    collections: serde_json::Value,
    pages: HashMap<(u64, u64), (serde_json::Value, bool, u64)>,
    audios: HashMap<u64, Vec<u8>>,
    fail_audio: HashSet<u64>,
    on_first_audio: Option<Arc<AtomicBool>>,
) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let url = format!("http://{addr}");
    let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let requests2 = requests.clone();
    let url2 = url.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(s) = stream else { continue };
            let collections = collections.clone();
            let pages = pages.clone();
            let audios = audios.clone();
            let fail_audio = fail_audio.clone();
            let flag = on_first_audio.clone();
            let requests = requests2.clone();
            let base = url2.clone();
            std::thread::spawn(move || {
                handle_conn(s, &collections, &pages, &audios, &fail_audio, &flag, &requests, &base);
            });
        }
    });
    Mock { url, requests }
}

#[allow(clippy::too_many_arguments)]
fn handle_conn(
    mut s: TcpStream,
    collections: &serde_json::Value,
    pages: &HashMap<(u64, u64), (serde_json::Value, bool, u64)>,
    audios: &HashMap<u64, Vec<u8>>,
    fail_audio: &HashSet<u64>,
    on_first_audio: &Option<Arc<AtomicBool>>,
    requests: &Mutex<Vec<String>>,
    base: &str,
) {
    // 读请求头 + 按 Content-Length 读 body。
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end;
    loop {
        match s.read(&mut tmp) {
            Ok(0) => return,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    header_end = pos + 4;
                    break;
                }
            }
            Err(_) => return,
        }
    }
    let head_text = String::from_utf8_lossy(&buf[..header_end]);
    let first = head_text.lines().next().unwrap_or("").to_string();
    let content_len: usize = head_text
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("content-length:"))
        .and_then(|l| l[15..].trim().parse().ok())
        .unwrap_or(0);
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_len {
        match s.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&tmp[..n]),
            Err(_) => break,
        }
    }
    let body_text = String::from_utf8_lossy(&body).into_owned();
    requests
        .lock()
        .expect("requests")
        .push(format!("{first} {body_text}"));

    let path = first.split_whitespace().nth(1).unwrap_or("");
    let respond = |s: &mut TcpStream, status: &str, bytes: &[u8]| {
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        );
        let _ = s.write_all(head.as_bytes());
        let _ = s.write_all(bytes);
    };

    if first.starts_with("POST") && path.starts_with("/lv/v1/get_collections") {
        let resp = serde_json::json!({"ret": "0", "errmsg": "success", "data": {"collections": collections}});
        respond(&mut s, "200 OK", resp.to_string().as_bytes());
        return;
    }
    if first.starts_with("POST") && path.starts_with("/lv/v1/get_collection_songs") {
        let req: serde_json::Value = serde_json::from_str(&body_text).unwrap_or_default();
        let id = req["id"].as_u64().unwrap_or(0);
        let cursor = req["cursor"].as_u64().unwrap_or(0);
        let Some((songs, has_more, next_offset)) = pages.get(&(id, cursor)) else {
            respond(&mut s, "200 OK", br#"{"ret":"0","data":{"songs":[],"has_more":false}}"#);
            return;
        };
        let songs_text = songs.to_string().replace("@BASE@", base);
        let resp = format!(
            r#"{{"ret":"0","errmsg":"success","data":{{"songs":{songs_text},"has_more":{has_more},"next_offset":{next_offset}}}}}"#
        );
        respond(&mut s, "200 OK", resp.as_bytes());
        return;
    }
    if first.starts_with("GET") && path.starts_with("/a/") {
        if let Some(flag) = on_first_audio {
            flag.store(true, Ordering::Relaxed);
        }
        let id: u64 = path[3..].split('?').next().unwrap_or("0").parse().unwrap_or(0);
        if fail_audio.contains(&id) {
            respond(&mut s, "500 Internal Server Error", b"");
            return;
        }
        match audios.get(&id) {
            Some(bytes) => respond(&mut s, "200 OK", bytes),
            None => respond(&mut s, "404 Not Found", b""),
        }
        return;
    }
    respond(&mut s, "404 Not Found", b"");
}

/// 假音频：m4a 头（ftyp）+ 填充到 >16KB；内容含 id 保证各首 md5 不同。
fn audio_bytes(id: u64) -> Vec<u8> {
    let mut v = b"\0\0\0\x18ftypM4A ".to_vec();
    v.extend_from_slice(format!("song-{id}-").as_bytes());
    v.resize(20 * 1024 + (id % 100) as usize, b'x');
    v
}

fn song_json(id: u64, title: &str, author: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "title": title,
        "author": author,
        "duration": 120,
        "cover_url": {"hd": format!("https://covers.example/{id}.jpg")},
        "preview_url": format!("@BASE@/a/{id}"),
    })
}

fn collections_json() -> serde_json::Value {
    serde_json::json!([
        {"id": CAT1, "name": "会员热榜"},
        {"id": CAT2, "name": "抖音热门"},
    ])
}

/// 标准场景：分类1 两页（page1: s1,s2 + has_more；page2: s2 重复,s3），分类2 一页（s3 跨分类重复,s4）。
fn standard_pages() -> HashMap<(u64, u64), (serde_json::Value, bool, u64)> {
    HashMap::from([
        (
            (CAT1, 0),
            (
                serde_json::json!([song_json(S1, "T1", "A1"), song_json(S2, "T2", "A2")]),
                true,
                2,
            ),
        ),
        (
            (CAT1, 2),
            (
                serde_json::json!([song_json(S2, "T2", "A2"), song_json(S3, "T3", "A3")]),
                false,
                4,
            ),
        ),
        (
            (CAT2, 0),
            (
                serde_json::json!([song_json(S3, "T3", "A3"), song_json(S4, "T4", "A4")]),
                false,
                2,
            ),
        ),
    ])
}

fn standard_audios() -> HashMap<u64, Vec<u8>> {
    HashMap::from([
        (S1, audio_bytes(S1)),
        (S2, audio_bytes(S2)),
        (S3, audio_bytes(S3)),
        (S4, audio_bytes(S4)),
    ])
}

fn test_env() -> (tempfile::TempDir, Arc<Database>, MediaStore, std::path::PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let db = Arc::new(Database::open(&tmp.path().join("db/app.db")).expect("db"));
    let store = MediaStore::new(tmp.path().to_path_buf());
    std::fs::create_dir_all(store.music_dir()).expect("music dir");
    let pp = tmp.path().join("progress.json");
    (tmp, db, store, pp)
}

fn read_progress(pp: &std::path::Path) -> CollectProgress {
    serde_json::from_str(&std::fs::read_to_string(pp).expect("progress file")).expect("parse")
}

fn mapping_rows(db: &Database) -> Vec<(String, String, String)> {
    let conn = db.conn();
    let mut stmt = conn
        .prepare("SELECT song_id, file_name, category FROM music_categories ORDER BY song_id, category")
        .expect("prepare");
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows")
}

fn audio_request_count(mock: &Mock) -> usize {
    mock.requests
        .lock()
        .expect("requests")
        .iter()
        .filter(|r| r.starts_with("GET /a/"))
        .count()
}

#[test]
fn collect_full_flow_pagination_dedup_and_mapping() {
    let (_tmp, db, store, pp) = test_env();
    // 预置「同内容已存在」：S2 的目标文件已存在且内容一致 → 下载后走 Skip 复用该文件名。
    std::fs::write(store.music_dir().join("T2 - A2.m4a"), audio_bytes(S2)).expect("seed");

    let mock = start_mock(collections_json(), standard_pages(), standard_audios(), HashSet::new(), None);
    let cancel = AtomicBool::new(false);
    run_collect(&db, &store, 3, &mock.url, &pp, &cancel).expect("run_collect");

    let p = read_progress(&pp);
    assert_eq!(p.stage, "done");
    assert_eq!(p.planned_total, 4, "跨分类重复的 S3 只计一次新下载");
    assert_eq!(p.downloaded, 4);
    assert_eq!(p.failed, 0);
    // 分类2 的 S3 走「已有映射」分支补分类行。
    assert_eq!(p.skipped, 1);

    // 文件落盘（S2 复用预置文件，不留 (2) 副本；无 .part 残留）。
    for name in ["T1 - A1.m4a", "T2 - A2.m4a", "T3 - A3.m4a", "T4 - A4.m4a"] {
        assert!(store.music_dir().join(name).is_file(), "缺文件 {name}");
    }
    assert!(!store.music_dir().join("T2 - A2 (2).m4a").exists());
    assert!(
        !std::fs::read_dir(store.music_dir())
            .expect("read_dir")
            .flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with(".part"))
    );

    // 分类映射 5 行；19 位 song_id 存 TEXT 不丢精度；一首歌多分类多行。
    let rows = mapping_rows(&db);
    assert_eq!(rows.len(), 5);
    assert!(rows.contains(&(S1.to_string(), "T1 - A1.m4a".to_string(), "会员热榜".to_string())));
    assert!(rows.contains(&(S3.to_string(), "T3 - A3.m4a".to_string(), "会员热榜".to_string())));
    assert!(rows.contains(&(S3.to_string(), "T3 - A3.m4a".to_string(), "抖音热门".to_string())));

    // S3 跨分类只下载一次：audio 请求共 4 次。
    assert_eq!(audio_request_count(&mock), 4);

    // 封面 URL 已落 music_covers（URL_READY 待下载）。
    let conn = db.conn();
    let cover_status: String = conn
        .query_row(
            "SELECT status FROM music_covers WHERE file_name = 'T1 - A1.m4a'",
            [],
            |r| r.get(0),
        )
        .expect("cover row");
    assert_eq!(cover_status, "URL_READY");
}

#[test]
fn collect_rerun_is_idempotent_all_skipped() {
    let (_tmp, db, store, pp) = test_env();
    let mock = start_mock(collections_json(), standard_pages(), standard_audios(), HashSet::new(), None);
    let cancel = AtomicBool::new(false);
    run_collect(&db, &store, 3, &mock.url, &pp, &cancel).expect("first run");
    let audio_after_first = audio_request_count(&mock);

    run_collect(&db, &store, 3, &mock.url, &pp, &cancel).expect("second run");
    let p = read_progress(&pp);
    assert_eq!(p.stage, "done");
    assert_eq!(p.downloaded, 0, "重跑不应有新下载");
    assert_eq!(p.planned_total, 0);
    assert_eq!(p.skipped, 5, "全部 5 行计划走已有映射跳过");
    assert_eq!(audio_request_count(&mock), audio_after_first, "重跑不发音频请求");
    assert_eq!(mapping_rows(&db).len(), 5);
}

#[test]
fn collect_stops_when_cancelled_mid_download() {
    let (_tmp, db, store, pp) = test_env();
    // 第一个音频请求到达时置取消标记 → 第二首歌前停止。
    let flag = Arc::new(AtomicBool::new(false));
    let mock = start_mock(
        collections_json(),
        standard_pages(),
        standard_audios(),
        HashSet::new(),
        Some(flag.clone()),
    );
    run_collect(&db, &store, 3, &mock.url, &pp, &flag).expect("run_collect");

    let p = read_progress(&pp);
    assert_eq!(p.stage, "stopped");
    assert_eq!(p.downloaded, 1, "只完成了第一首");
    assert!(store.music_dir().join("T1 - A1.m4a").is_file());
    assert!(!store.music_dir().join("T4 - A4.m4a").exists());
}

#[test]
fn collect_skips_failed_song_and_continues() {
    let (_tmp, db, store, pp) = test_env();
    let fail = HashSet::from([S2]);
    let mock = start_mock(collections_json(), standard_pages(), standard_audios(), fail, None);
    let cancel = AtomicBool::new(false);
    run_collect(&db, &store, 3, &mock.url, &pp, &cancel).expect("run_collect");

    let p = read_progress(&pp);
    assert_eq!(p.stage, "done");
    assert_eq!(p.downloaded, 3);
    assert_eq!(p.failed, 1);
    assert!(!store.music_dir().join("T2 - A2.m4a").exists());
    // 失败的歌不落映射，其余正常。
    let rows = mapping_rows(&db);
    assert_eq!(rows.len(), 4);
    assert!(!rows.iter().any(|(sid, _, _)| sid == &S2.to_string()));
}

#[test]
fn collect_errors_when_collections_fetch_fails() {
    let (_tmp, db, store, pp) = test_env();
    // mock 没有任何 pages/collections 以外的路由问题——直接给空分类列表。
    let mock = start_mock(serde_json::json!([]), HashMap::new(), HashMap::new(), HashSet::new(), None);
    let cancel = AtomicBool::new(false);
    let r = run_collect(&db, &store, 3, &mock.url, &pp, &cancel);
    assert!(r.is_err());
    assert_eq!(read_progress(&pp).stage, "error");
}
