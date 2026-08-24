//! DouyinSync Agent 集成测试：用假同步 sidecar 验证「同步完成 -> posts.jsonl 幂等入库 -> 计数」。
use std::path::{Path, PathBuf};

use build_douyin_acount_lib::agents::douyin_sync::DouyinSyncAgent;
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

struct TestEnv {
    _dir: tempfile::TempDir,
    db: Database,
    store: MediaStore,
    agent: DouyinSyncAgent,
    task_id: i64,
}

fn setup() -> TestEnv {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(&dir.path().join("test.db")).expect("db");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("dirs");

    let task_id = TasksRepo::new(&db)
        .enqueue("douyin_sync", "sync_works", json!({}), false)
        .expect("enqueue");
    let agent = DouyinSyncAgent::new("node", fixture("fake_douyin_sync.cjs"));
    TestEnv {
        _dir: dir,
        db,
        store,
        agent,
        task_id,
    }
}

fn claim(env: &TestEnv) -> build_douyin_acount_lib::domain::task::AgentTask {
    TasksRepo::new(&env.db)
        .claim_next("douyin_sync")
        .expect("claim")
        .expect("some task")
}

fn post(env: &TestEnv, aweme_id: &str) -> build_douyin_acount_lib::domain::douyin::DouyinPost {
    DouyinPostsRepo::new(&env.db)
        .list(10)
        .expect("list")
        .into_iter()
        .find(|p| p.aweme_id == aweme_id)
        .unwrap_or_else(|| panic!("post {aweme_id} should exist"))
}

#[test]
fn douyin_sync_upserts_posts_and_reports_counts() {
    let env = setup();
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    // 第一轮：spawn sidecar -> Continue
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run 1");
    assert!(matches!(outcome, AgentOutcome::Continue));
    // 后续轮次：sidecar 完成 -> 入库并成功
    let mut outcome = None;
    for _ in 0..20 {
        let task = claim(&env);
        outcome = Some(env.agent.run(&ctx, task).expect("run"));
        if !matches!(outcome.as_ref().unwrap(), AgentOutcome::Continue) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    match outcome.expect("ran") {
        AgentOutcome::Succeeded(v) => {
            assert_eq!(v["synced"], 3);
            assert_eq!(v["scheduled"], 1);
            assert_eq!(v["violations"], 1);
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }
    let repo = DouyinPostsRepo::new(&env.db);
    assert_eq!(repo.count().expect("count"), 3);
    let published = post(&env, "fake_dy_1");
    assert_eq!(published.status, "published");
    assert_eq!(published.play_count, 1234);
    assert_eq!(published.digg_count, 56);
    // 率类指标来自 items[].metrics，随列表一并入库。
    assert_eq!(published.completion_rate, Some(23.99));
    assert_eq!(published.bounce_rate_2s, Some(44.87));
    let scheduled = post(&env, "fake_dy_2");
    assert_eq!(scheduled.status, "scheduled");
    // 定时作品的 create_time 即定时发布时间（恒定未来）。
    assert_eq!(scheduled.create_time, 4102444800);
    assert!(scheduled.completion_rate.is_none());
    // 违规作品的审核摘要/原因/状态随同步入库。
    let violating = post(&env, "fake_dy_3");
    assert_eq!(violating.status, "published");
    assert_eq!(violating.violation_status, Some(5));
    assert_eq!(
        violating.violation_text.as_deref(),
        Some("作品流量减少，优化后可以获得更多流量")
    );
    assert_eq!(violating.violation_brief.as_deref(), Some("原创性不足"));
    // 违规原因 id 清单供 sidecar 跳过详情补抓。
    assert_eq!(
        repo.list_violation_brief_ids().expect("ids"),
        vec!["fake_dy_3".to_string()]
    );
}

#[test]
fn douyin_sync_repo_upsert_updates_stats_and_keeps_old_cover() {
    // 二次同步语义：同 aweme_id 行数不变、统计刷新；本轮没下到封面时保留旧封面。
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(&dir.path().join("test.db")).expect("db");
    let repo = DouyinPostsRepo::new(&db);
    let base = NewDouyinPost {
        aweme_id: "dy_1".to_string(),
        title: Some("标题".to_string()),
        cover_path: Some("covers/douyin_dy_1.webp".to_string()),
        cover_url: Some("https://example.com/c1.webp".to_string()),
        status: "published".to_string(),
        play_count: 100,
        digg_count: 10,
        comment_count: 1,
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
    repo.upsert(&base).expect("first upsert");
    let refreshed = NewDouyinPost {
        play_count: 200,
        digg_count: 20,
        cover_path: None, // 本轮封面下载失败
        ..base.clone()
    };
    repo.upsert(&refreshed).expect("second upsert");
    assert_eq!(repo.count().expect("count"), 1, "upsert 必须幂等");
    let rows = repo.list(10).expect("list");
    assert_eq!(rows[0].play_count, 200, "统计应被刷新");
    assert_eq!(rows[0].digg_count, 20);
    assert_eq!(
        rows[0].cover_path.as_deref(),
        Some("covers/douyin_dy_1.webp"),
        "新值为空时应保留旧封面"
    );
}

#[test]
fn douyin_sync_repo_violation_cleared_when_platform_lifts_it() {
    // 违规语义：status/text 以本轮列表为准（覆盖式）；brief 本轮没补抓到保留旧值；
    // 一旦平台侧违规消解（本轮不再上报），全部违规字段必须清空。
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(&dir.path().join("test.db")).expect("db");
    let repo = DouyinPostsRepo::new(&db);
    let violating = NewDouyinPost {
        aweme_id: "dy_v".to_string(),
        title: None,
        cover_path: None,
        cover_url: None,
        status: "published".to_string(),
        play_count: 1,
        digg_count: 0,
        comment_count: 0,
        share_count: 0,
        completion_rate: None,
        bounce_rate_2s: None,
        violation_status: Some(5),
        violation_text: Some("作品流量减少".to_string()),
        violation_brief: Some("原创性不足".to_string()),
        violation_url: Some("https://example.com/appeal".to_string()),
        create_time: 1785000000,
        public_time: 0,
    };
    repo.upsert(&violating).expect("upsert violating");
    // 再次同步：仍在违规但本轮跳过详情补抓（brief 为空）→ 保留旧 brief。
    let still_violating = NewDouyinPost {
        violation_brief: None,
        violation_url: None,
        ..violating.clone()
    };
    repo.upsert(&still_violating).expect("upsert still violating");
    let row = &repo.list(1).expect("list")[0];
    assert_eq!(row.violation_brief.as_deref(), Some("原创性不足"), "跳过补抓时保留旧 brief");
    // 平台消解违规：本轮上报不再有违规字段 → 全部清空。
    let lifted = NewDouyinPost {
        violation_status: None,
        violation_text: None,
        violation_brief: None,
        violation_url: None,
        ..violating
    };
    repo.upsert(&lifted).expect("upsert lifted");
    let row = &repo.list(1).expect("list")[0];
    assert!(row.violation_status.is_none());
    assert!(row.violation_text.is_none());
    assert!(row.violation_brief.is_none(), "违规消解后 brief 必须清空");
    assert!(row.violation_url.is_none());
}

#[test]
fn douyin_sync_surfaces_failure() {
    let env = setup();
    let dir = env
        .store
        .root()
        .join("douyin_sync")
        .join(env.task_id.to_string());
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(
        dir.join("progress.json"),
        serde_json::to_string(&json!({ "state": "failed", "message": "boom" })).unwrap(),
    )
    .expect("progress");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let err = env.agent.run(&ctx, task).expect_err("must fail");
    assert!(err.to_string().contains("boom"), "{err}");
    // 可重试：临时目录已清空，下轮经 Missing 重拉 sidecar。
    assert!(!dir.join("progress.json").exists(), "失败后应清空临时目录");
}

#[test]
fn douyin_sync_maps_sidecar_cancelled_to_cancelled_outcome() {
    // 用户关闭浏览器标签页（sidecar 上报 cancelled）-> AgentOutcome::Cancelled，终态不重试。
    let env = setup();
    let dir = env
        .store
        .root()
        .join("douyin_sync")
        .join(env.task_id.to_string());
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(
        dir.join("progress.json"),
        serde_json::to_string(&json!({ "state": "cancelled", "message": "浏览器标签页已关闭" }))
            .unwrap(),
    )
    .expect("progress");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Cancelled(_)), "{outcome:?}");
}

#[test]
fn douyin_sync_respawns_when_sidecar_heartbeat_stale() {
    // 僵尸场景：progress.json 显示 running，但 mtime 停在 2 分钟前（sidecar 已被应用重启带走）。
    let env = setup();
    let dir = env
        .store
        .root()
        .join("douyin_sync")
        .join(env.task_id.to_string());
    std::fs::create_dir_all(&dir).expect("dir");
    let progress = dir.join("progress.json");
    std::fs::write(
        &progress,
        serde_json::to_string(&json!({ "state": "running", "synced": 0, "message": "stale" }))
            .unwrap(),
    )
    .expect("progress");
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&progress)
        .expect("open");
    file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(120))
        .expect("set mtime");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    // 心跳超时 -> 走失败分支：清目录、消耗一次尝试（不在本轮重拉，保证串行）。
    let err = env.agent.run(&ctx, task).expect_err("stale must fail");
    assert!(err.to_string().contains("无心跳"), "{err}");
    assert!(!progress.exists(), "失败后应清空临时目录");
    // 下一轮认领：progress.json 缺失（Missing）-> 重拉 sidecar。
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run 2");
    assert!(matches!(outcome, AgentOutcome::Continue));
    // 假 sidecar 应已被重新拉起并写出新的进度。
    let mut respawned = false;
    for _ in 0..30 {
        if let Ok(raw) = std::fs::read_to_string(&progress) {
            if raw.contains("fake sync") {
                respawned = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    assert!(respawned, "sidecar should have been respawned");
}
