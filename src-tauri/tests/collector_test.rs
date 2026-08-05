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
    let agent = CollectorAgent::new("node", fixture("fake_collect.cjs"), "yt-dlp");
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
