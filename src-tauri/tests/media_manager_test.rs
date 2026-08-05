use std::path::PathBuf;
use std::process::Command;

use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::domain::asset::AssetStatus;
use build_douyin_acount_lib::media::{ImportOptions, MediaStore, VideoManager};

fn has_tool(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 用 ffmpeg 生成一段 2 秒的测试视频，返回路径。
fn make_test_video(dir: &std::path::Path) -> Option<PathBuf> {
    if !has_tool("ffmpeg") {
        return None;
    }
    let out = dir.join("sample.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=320x240:rate=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&out)
        .output()
        .ok()?;
    if status.status.success() {
        Some(out)
    } else {
        None
    }
}

fn setup() -> (tempfile::TempDir, Database, MediaStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("dirs");
    let db = Database::open(&dir.path().join("storage/db/app.db")).expect("db");
    (dir, db, store)
}

fn default_import() -> ImportOptions {
    ImportOptions {
        source_platform: "tiktok".to_string(),
        title: Some("测试视频".to_string()),
        source_url: Some("https://tiktok.com/x".to_string()),
        author: Some("author".to_string()),
        dedup_key: Some("tiktok:media1".to_string()),
    }
}

#[test]
fn media_store_creates_directory_layout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("dirs");
    for sub in ["videos", "covers", "frames", "audio", "logs", "db"] {
        assert!(dir.path().join("storage").join(sub).is_dir(), "missing {sub}");
    }
}

#[test]
fn video_manager_imports_file_and_generates_cover() {
    let (dir, db, store) = setup();
    let mgr = VideoManager::new(&db, store);
    let Some(src) = make_test_video(dir.path()) else {
        eprintln!("skipped: ffmpeg not available");
        return;
    };
    let asset = mgr.import_file(&src, default_import()).expect("import");
    assert_eq!(asset.status, AssetStatus::New);
    assert_eq!(asset.source_platform, "tiktok");
    assert!(asset.duration_ms > 0, "duration probed: {}", asset.duration_ms);
    assert!(std::path::Path::new(&asset.file_path).exists(), "video file exists");
    assert!(
        asset.cover_path.as_deref().map(std::path::Path::new).is_some_and(|p| p.exists()),
        "cover file exists"
    );
    // 去重键冲突时拒绝导入
    let err = mgr.import_file(&src, default_import()).expect_err("dup rejected");
    assert!(err.to_string().contains("UNIQUE"), "got: {err}");
}

#[test]
fn video_manager_delete_removes_db_row_and_files() {
    let (dir, db, store) = setup();
    let mgr = VideoManager::new(&db, store);
    let Some(src) = make_test_video(dir.path()) else {
        eprintln!("skipped: ffmpeg not available");
        return;
    };
    let asset = mgr.import_file(&src, default_import()).expect("import");
    let video_path = asset.file_path.clone();
    let cover_path = asset.cover_path.clone().expect("cover");
    mgr.delete_asset(asset.id).expect("delete");
    assert!(!std::path::Path::new(&video_path).exists(), "video removed");
    assert!(!std::path::Path::new(&cover_path).exists(), "cover removed");
    assert!(mgr.get_asset(asset.id).expect("get").is_none());
}

#[test]
fn video_manager_favorite_and_status_flow() {
    let (dir, db, store) = setup();
    let mgr = VideoManager::new(&db, store);
    let Some(src) = make_test_video(dir.path()) else {
        eprintln!("skipped: ffmpeg not available");
        return;
    };
    let asset = mgr.import_file(&src, default_import()).expect("import");

    mgr.set_favorite(asset.id, true).expect("fav");
    assert!(mgr.get_asset(asset.id).expect("get").expect("some").is_favorite);

    // NEW -> ANALYZING -> READY -> SELECTED -> PUBLISHED 全链路合法
    mgr.update_status(asset.id, AssetStatus::Analyzing).expect("analyzing");
    mgr.update_status(asset.id, AssetStatus::Candidate).expect("CANDIDATE");
    mgr.update_status(asset.id, AssetStatus::Selected).expect("selected");
    mgr.update_status(asset.id, AssetStatus::Published).expect("published");
    assert_eq!(
        mgr.get_asset(asset.id).expect("get").expect("some").status,
        AssetStatus::Published
    );
}

#[test]
fn video_manager_rejects_illegal_status_jump() {
    let (dir, db, store) = setup();
    let mgr = VideoManager::new(&db, store);
    let Some(src) = make_test_video(dir.path()) else {
        eprintln!("skipped: ffmpeg not available");
        return;
    };
    let asset = mgr.import_file(&src, default_import()).expect("import");
    let err = mgr
        .update_status(asset.id, AssetStatus::Published)
        .expect_err("NEW -> PUBLISHED must fail");
    assert!(err.to_string().contains("transition"), "got: {err}");
}
