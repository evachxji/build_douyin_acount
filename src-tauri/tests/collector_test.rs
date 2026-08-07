//! Collector Agent 集成测试：用假采集 sidecar 验证「采集完成 -> 导入素材库 -> 入队分析」。
use std::path::{Path, PathBuf};

use build_douyin_acount_lib::agents::collector::CollectorAgent;
use build_douyin_acount_lib::agents::{Agent, AgentContext, AgentOutcome};
use build_douyin_acount_lib::db::repos::assets::AssetsRepo;
use build_douyin_acount_lib::db::repos::tasks::TasksRepo;
use build_douyin_acount_lib::db::Database;
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
    agent: CollectorAgent,
    task_id: i64,
}

fn setup(count: i64) -> TestEnv {
    setup_with(count, "fake_collect.cjs")
}

fn setup_with(count: i64, script: &str) -> TestEnv {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(&dir.path().join("test.db")).expect("db");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("dirs");

    let task_id = TasksRepo::new(&db)
        .enqueue(
            "collector",
            "collect_batch",
            json!({ "platform": "tiktok", "count": count, "maxLikes": 1_000_000 }),
            false,
        )
        .expect("enqueue");
    let agent = CollectorAgent::new("node", fixture(script), "yt-dlp");
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
        .claim_next("collector")
        .expect("claim")
        .expect("some task")
}

fn preplace_fake_videos(env: &TestEnv) {
    // 假 sidecar 的 videos.jsonl 会引用 outputDir/fakeN.mp4，测试先放好真实小视频。
    let out = env
        .store
        .root()
        .join("collect")
        .join(env.task_id.to_string())
        .join("output");
    std::fs::create_dir_all(&out).expect("output dir");
    let sample = fixture("sample_1s.mp4");
    std::fs::copy(&sample, out.join("fake1.mp4")).expect("copy fake1");
    std::fs::copy(&sample, out.join("fake2.mp4")).expect("copy fake2");
}

#[test]
fn collector_imports_downloaded_videos_and_enqueues_analysis() {
    let env = setup(2);
    preplace_fake_videos(&env);
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    // 第一轮：spawn sidecar -> Continue
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run 1");
    assert!(matches!(outcome, AgentOutcome::Continue));
    // 后续轮次：sidecar 完成 -> 导入并成功
    let mut outcome = None;
    for _ in 0..20 {
        let task = claim(&env);
        outcome = Some(env.agent.run(&ctx, task).expect("run"));
        if !matches!(outcome.as_ref().unwrap(), AgentOutcome::Continue) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let outcome = outcome.expect("ran");
    match outcome {
        AgentOutcome::Succeeded(v) => {
            assert_eq!(v["imported"], 2);
            assert_eq!(v["enqueued_analysis"], 2);
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }
    // 素材已入库（dedup_key 平台:id），状态 READY 并带互动数据
    let assets = AssetsRepo::new(&env.db);
    let a = assets
        .find_by_dedup("tiktok:fake1")
        .expect("find")
        .expect("fake1 asset");
    assert_eq!(a.likes, 100);
    assert_eq!(a.plays, 200);
    assert_eq!(a.source_platform, "tiktok");
    // 分析任务已入队
    let vision = TasksRepo::new(&env.db)
        .list(None)
        .expect("tasks")
        .into_iter()
        .filter(|t| t.agent_name == "vision" && t.task_type == "analyze")
        .count();
    assert_eq!(vision, 2);
}

#[test]
fn collector_imported_is_task_total_not_final_round() {
    // 回归：真实 sidecar 逐条追加 videos.jsonl，Agent 在 RUNNING 阶段已增量导入大部分，
    // Done 时若只按"本轮新导入"计数，会把全任务累计数算成 0/1。imported 必须是全任务累计。
    let env = setup_with(3, "fake_collect_batch.cjs");
    let out = env
        .store
        .root()
        .join("collect")
        .join(env.task_id.to_string())
        .join("output");
    std::fs::create_dir_all(&out).expect("output dir");
    let sample = fixture("sample_1s.mp4");
    for i in 1..=3 {
        std::fs::copy(&sample, out.join(format!("fake{i}.mp4"))).expect("copy fake video");
    }
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let mut outcome = None;
    for _ in 0..30 {
        let task = claim(&env);
        outcome = Some(env.agent.run(&ctx, task).expect("run"));
        if !matches!(outcome.as_ref().unwrap(), AgentOutcome::Continue) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    match outcome.expect("ran") {
        AgentOutcome::Succeeded(v) => {
            assert_eq!(v["imported"], 3, "imported 应为全任务累计 3 条，而非最终轮新增");
            assert_eq!(v["first_asset"]["title"], "fake 1");
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }
    // 三条均入库。
    let assets = AssetsRepo::new(&env.db);
    for i in 1..=3 {
        assert!(
            assets.find_by_dedup(&format!("tiktok:fake{i}")).expect("find").is_some(),
            "fake{i} 应已入库"
        );
    }
}

#[test]
fn collector_surfaces_failure() {
    let env = setup(1);
    let dir = env
        .store
        .root()
        .join("collect")
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
}

#[test]
fn collector_maps_sidecar_cancelled_to_cancelled_outcome() {
    // 用户关闭浏览器标签页（sidecar 上报 cancelled）-> AgentOutcome::Cancelled，终态不重试。
    let env = setup(1);
    let dir = env
        .store
        .root()
        .join("collect")
        .join(env.task_id.to_string());
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(
        dir.join("progress.json"),
        serde_json::to_string(&json!({ "state": "cancelled", "message": "浏览器标签页已关闭" })).unwrap(),
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
fn collector_respawns_when_sidecar_heartbeat_stale() {
    // 僵尸场景：progress.json 显示 running，但 mtime 停在 2 分钟前（sidecar 已被应用重启带走）。
    let env = setup(1);
    let dir = env
        .store
        .root()
        .join("collect")
        .join(env.task_id.to_string());
    std::fs::create_dir_all(&dir).expect("dir");
    let progress = dir.join("progress.json");
    std::fs::write(
        &progress,
        serde_json::to_string(
            &json!({ "state": "running", "downloaded": 0, "target": 1, "message": "stale" }),
        )
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
            if raw.contains("fake collect") {
                respawned = true;
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    assert!(respawned, "sidecar should have been respawned");
}
