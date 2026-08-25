//! DouyinExport Agent 集成测试：用假同步 sidecar 验证「拉取 -> 入库 -> 生成 Excel -> 返回文件路径」。
use std::path::{Path, PathBuf};

use build_douyin_acount_lib::agents::douyin_export::DouyinExportAgent;
use build_douyin_acount_lib::agents::{Agent, AgentContext, AgentOutcome};
use build_douyin_acount_lib::db::repos::douyin_posts::DouyinPostsRepo;
use build_douyin_acount_lib::db::repos::tasks::TasksRepo;
use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::domain::douyin::NewDouyinPost;
use build_douyin_acount_lib::media::MediaStore;
use serde_json::json;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

#[test]
fn douyin_export_generates_excel_after_sync() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(&dir.path().join("test.db")).expect("db");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("dirs");

    TasksRepo::new(&db)
        .enqueue("douyin_export", "export_works", json!({}), false)
        .expect("enqueue");    let agent = DouyinExportAgent::new("node", fixture("fake_douyin_sync.cjs"));
    let ctx = AgentContext {
        db: &db,
        store: store.clone(),
    };

    // 第一轮：spawn sidecar -> Continue
    let task = TasksRepo::new(&db).claim_next("douyin_export").expect("claim").expect("task");
    let outcome = agent.run(&ctx, task).expect("run 1");
    assert!(matches!(outcome, AgentOutcome::Continue));

    // 后续轮次：sidecar 完成 -> 入库并生成 Excel
    let mut outcome = None;
    for _ in 0..20 {
        let task = TasksRepo::new(&db)
            .claim_next("douyin_export")
            .expect("claim")
            .expect("task");
        outcome = Some(agent.run(&ctx, task).expect("run"));
        if !matches!(outcome.as_ref().unwrap(), AgentOutcome::Continue) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    match outcome.expect("ran") {
        AgentOutcome::Succeeded(v) => {
            // 假数据 3 条全是 published/scheduled，全部进入导出清单。
            assert_eq!(v["rows"], 3);
            let fp = v["file_path"].as_str().expect("file_path");
            let p = Path::new(fp);
            assert!(p.exists(), "导出的 xlsx 应存在");
            let bytes = std::fs::read(p).expect("read xlsx");
            assert!(bytes.starts_with(b"PK"), "xlsx 应为 zip 容器");
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }
}

#[test]
fn list_for_export_only_includes_published_and_scheduled() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(&dir.path().join("test.db")).expect("db");
    let repo = DouyinPostsRepo::new(&db);
    let mk = |aweme: &str, status: &str| NewDouyinPost {
        aweme_id: aweme.to_string(),
        title: None,
        cover_path: None,
        cover_url: None,
        status: status.to_string(),
        play_count: 0,
        digg_count: 0,
        comment_count: 0,
        share_count: 0,
        completion_rate: None,
        bounce_rate_2s: None,
        violation_status: None,
        violation_text: None,
        violation_brief: None,
        violation_url: None,
        create_time: 1785000000,
        public_time: 0,
    };
    for (aweme, status) in [
        ("a", "published"),
        ("b", "scheduled"),
        ("c", "private"),
        ("d", "reviewing"),
        ("e", "deleted"),
    ] {
        repo.upsert(&mk(aweme, status)).expect("upsert");
    }
    let rows = repo.list_for_export().expect("list_for_export");
    let ids: Vec<&str> = rows.iter().map(|p| p.aweme_id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b"], "仅导出已发布 + 定时待发布");
}
