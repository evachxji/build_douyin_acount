//! 备份导出/导入集成测试：导出 zip -> 覆盖式导入（暂存 + 应用）的完整回路。
use std::path::Path;

use build_douyin_acount_lib::backup;
use build_douyin_acount_lib::db::repos::assets::AssetsRepo;
use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::domain::asset::NewAsset;
use build_douyin_acount_lib::media::MediaStore;

fn make_env(dir: &Path) -> (Database, MediaStore) {
    let store = MediaStore::new(dir.join("storage"));
    store.ensure_dirs().expect("dirs");
    let db = Database::open(&store.db_dir().join("app.db")).expect("db");
    (db, store)
}

fn insert_asset(db: &Database, dedup: &str, title: &str) {
    AssetsRepo::new(db)
        .insert(NewAsset {
            dedup_key: Some(dedup.to_string()),
            file_path: format!("videos/{dedup}.mp4"),
            cover_path: None,
            source_platform: "tiktok".to_string(),
            source_url: None,
            author: None,
            title: Some(title.to_string()),
            description: None,
            published_at: None,
            tags: vec![],
            likes: 0,
            comments: 0,
            plays: 0,
            duration_ms: 1000,
        })
        .expect("insert asset");
}

#[test]
fn export_then_import_roundtrip() {
    let dir_a = tempfile::tempdir().expect("dir a");
    let (db_a, store_a) = make_env(dir_a.path());
    insert_asset(&db_a, "tt:1", "视频一");
    insert_asset(&db_a, "tt:2", "视频二");
    std::fs::write(store_a.videos_dir().join("a1.mp4"), b"video-one").expect("video 1");
    std::fs::write(store_a.videos_dir().join("a2.mp4"), b"video-two").expect("video 2");
    std::fs::write(store_a.covers_dir().join("a1.jpg"), b"cover-one").expect("cover");
    std::fs::write(store_a.root().join("cookies.txt"), "session=abc").expect("cookies");

    let dest = dir_a.path().join("backup.zip");
    let stats = backup::export_backup(&store_a, &db_a, &dest).expect("export");
    assert_eq!(stats.files, 6, "manifest + db + 2 videos + 1 cover + cookies");
    assert!(stats.bytes > 0);
    // 源库/源文件保持原样（导出是只读操作）。
    assert!(store_a.videos_dir().join("a1.mp4").exists());

    // zip 内容结构校验。
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&dest).expect("open zip")).expect("zip");
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).expect("entry").name().to_string())
        .collect();
    for expect in ["manifest.json", "db/app.db", "videos/a1.mp4", "videos/a2.mp4", "covers/a1.jpg", "cookies.txt"] {
        assert!(names.iter().any(|n| n == expect), "zip 应包含 {expect}，实际: {names:?}");
    }

    // 目标环境：有会被覆盖的旧数据。
    let dir_b = tempfile::tempdir().expect("dir b");
    let (db_b, store_b) = make_env(dir_b.path());
    insert_asset(&db_b, "old:9", "旧素材");
    std::fs::write(store_b.videos_dir().join("old.mp4"), b"old-video").expect("old video");
    std::fs::write(store_b.root().join("cookies.txt"), "session=old").expect("old cookies");
    drop(db_b); // 模拟重启：应用导入前进程不持有 db 句柄。

    let preview = backup::stage_import(&store_b, &dest).expect("stage");
    assert_eq!(preview.assets, 2);
    assert_eq!(preview.videos, 2);
    // stage 只解压校验，不写标记——取消导入不应产生副作用。
    assert!(!backup::apply_pending_import(&store_b).expect("apply none"), "无标记时不应应用");

    backup::mark_import_pending(&store_b).expect("mark");
    assert!(backup::apply_pending_import(&store_b).expect("apply"), "有标记应执行导入");

    // 覆盖语义：旧素材/旧视频消失，zip 内容完整落地。
    let db_b2 = Database::open(&store_b.db_dir().join("app.db")).expect("reopen db");
    let repo = AssetsRepo::new(&db_b2);
    assert!(repo.find_by_dedup("tt:1").expect("find").is_some());
    assert!(repo.find_by_dedup("tt:2").expect("find").is_some());
    assert!(repo.find_by_dedup("old:9").expect("find").is_none(), "旧素材应被覆盖清除");
    assert!(store_b.videos_dir().join("a1.mp4").exists());
    assert!(!store_b.videos_dir().join("old.mp4").exists(), "旧视频应被覆盖清除");
    assert_eq!(
        std::fs::read_to_string(store_b.root().join("cookies.txt")).expect("cookies"),
        "session=abc"
    );
    // 标记与暂存已清理（重复启动不会重复应用）。
    assert!(!backup::apply_pending_import(&store_b).expect("apply again"));
}

#[test]
fn import_rejects_foreign_zip() {
    let dir = tempfile::tempdir().expect("dir");
    let (_db, store) = make_env(dir.path());
    // 有 zip 结构但不是本应用备份（无 manifest.json）。
    let foreign = dir.path().join("foreign.zip");
    {
        let mut zw = zip::ZipWriter::new(std::fs::File::create(&foreign).expect("create"));
        zw.start_file("readme.txt", zip::write::SimpleFileOptions::default())
            .expect("start");
        use std::io::Write;
        zw.write_all(b"hello").expect("write");
        zw.finish().expect("finish");
    }
    let err = backup::stage_import(&store, &foreign).expect_err("must reject");
    assert!(err.to_string().contains("manifest"), "{err}");
    assert!(
        !backup::apply_pending_import(&store).expect("apply"),
        "被拒绝的包不应留下 pending 标记"
    );
}

#[test]
fn apply_pending_import_keeps_data_when_no_marker() {
    let dir = tempfile::tempdir().expect("dir");
    let (_db, store) = make_env(dir.path());
    std::fs::write(store.videos_dir().join("keep.mp4"), b"keep").expect("video");
    assert!(!backup::apply_pending_import(&store).expect("apply"));
    assert!(store.videos_dir().join("keep.mp4").exists(), "无标记时数据不动");
}

/// 造一个手工暂存区（跳过解压环节，直接测 apply）：新 videos + 真库 db。
fn make_staging(store: &MediaStore) {
    let staging = store.root().join(".import_staging");
    std::fs::create_dir_all(staging.join("videos")).expect("staging videos");
    std::fs::create_dir_all(staging.join("db")).expect("staging db");
    std::fs::write(staging.join("videos").join("new.mp4"), b"new").expect("new video");
    let staged_db = Database::open(&staging.join("db").join("app.db")).expect("staged db");
    drop(staged_db);
    std::fs::write(store.root().join(".import_pending"), "pending").expect("mark");
}

#[cfg(windows)]
#[test]
fn apply_retries_while_target_file_locked_by_old_process() {
    // 回归：app.restart() 先起新进程、旧进程后退出，旧进程持有的 db 句柄让 rename
    // 报共享冲突（os error 32/5），导入静默失败。apply 必须等待重试，句柄释放后完成替换。
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().expect("dir");
    let (db, store) = make_env(dir.path());
    std::fs::write(store.videos_dir().join("old.mp4"), b"old").expect("old video");
    drop(db); // 本进程不持有句柄，只留"旧进程"的锁
    make_staging(&store);

    // 模拟旧进程未退：独占锁定现有 db（share_mode(0) 拒绝 rename 所需的 DELETE 共享）。
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(store.db_dir().join("app.db"))
        .expect("lock db");
    let handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        drop(lock); // 旧进程退出，句柄释放
    });

    assert!(backup::apply_pending_import(&store).expect("apply"), "应执行导入");
    handle.join().expect("join");
    assert!(store.videos_dir().join("new.mp4").exists(), "新视频应就位");
    assert!(!store.videos_dir().join("old.mp4").exists(), "旧视频应被覆盖");
    assert!(!store.root().join(".import_pending").exists(), "成功后标记已清");
    let db2 = Database::open(&store.db_dir().join("app.db")).expect("reopen");
    assert_eq!(AssetsRepo::new(&db2).list(Default::default()).expect("list").len(), 0);
}

#[cfg(windows)]
#[test]
fn apply_keeps_mark_and_staging_when_target_locked_for_good() {
    // 占用一直不释放（重试 5s 上限耗尽）→ 失败回滚，但标记与暂存保留，下次启动可重试。
    use std::os::windows::fs::OpenOptionsExt;
    let dir = tempfile::tempdir().expect("dir");
    let (db, store) = make_env(dir.path());
    std::fs::write(store.videos_dir().join("old.mp4"), b"old").expect("old video");
    drop(db);
    make_staging(&store);

    let _lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(store.db_dir().join("app.db"))
        .expect("lock db");
    let err = backup::apply_pending_import(&store).expect_err("锁定未释放必须失败");
    assert!(err.to_string().contains("os error"), "{err}");
    // 现场保留：标记与暂存还在（下次启动重试），原数据已回滚。
    assert!(store.root().join(".import_pending").exists(), "失败后标记应保留");
    assert!(store.root().join(".import_staging").join("videos").join("new.mp4").exists(), "暂存应完整保留");
    assert!(store.videos_dir().join("old.mp4").exists(), "原数据应已回滚");
    assert!(store.logs_dir().join("import_error.txt").exists(), "错误应落盘");
}
