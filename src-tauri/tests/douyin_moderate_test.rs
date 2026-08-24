//! DouyinModerate Agent 集成测试：job 装配（违规清单/定时队列）与结果收敛、自动补同步。
use std::path::{Path, PathBuf};

use build_douyin_acount_lib::agents::douyin_moderate::DouyinModerateAgent;
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
    agent: DouyinModerateAgent,
    task_id: i64,
}

fn post(aweme_id: &str, create_time: i64, status: &str, violation: bool) -> NewDouyinPost {
    NewDouyinPost {
        aweme_id: aweme_id.to_string(),
        title: Some(format!("post {aweme_id}")),
        cover_path: None,
        cover_url: None,
        status: status.to_string(),
        play_count: 0,
        digg_count: 0,
        comment_count: 0,
        share_count: 0,
        completion_rate: None,
        bounce_rate_2s: None,
        violation_status: if violation { Some(5) } else { None },
        violation_text: if violation {
            Some("作品流量减少".to_string())
        } else {
            None
        },
        violation_brief: None,
        violation_url: None,
        create_time,
        public_time: 0,
    }
}

fn setup(payload: serde_json::Value) -> TestEnv {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(&dir.path().join("test.db")).expect("db");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("dirs");

    // 数据：今天发布的违规、昨天发布的违规、已私密的违规（不再处理）、正常已发布、最晚定时、较早定时。
    let now = chrono::Utc::now().timestamp();
    let repo = DouyinPostsRepo::new(&db);
    repo.upsert(&post("vio_today", now - 3600, "published", true)).expect("seed");
    repo.upsert(&post("vio_old", now - 30 * 86400, "published", true)).expect("seed");
    repo.upsert(&post("vio_private", now - 5 * 86400, "private", true)).expect("seed");
    repo.upsert(&post("normal", now - 2 * 86400, "published", false)).expect("seed");
    repo.upsert(&post("sch_late", now + 3 * 86400, "scheduled", false)).expect("seed");
    repo.upsert(&post("sch_early", now + 86400, "scheduled", false)).expect("seed");

    let task_id = TasksRepo::new(&db)
        .enqueue("douyin_moderate", "moderate_violation", payload, false)
        .expect("enqueue");
    let agent = DouyinModerateAgent::new("node", fixture("fake_moderate.cjs"));
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
        .claim_next("douyin_moderate")
        .expect("claim")
        .expect("some task")
}

fn run_until_terminal(env: &TestEnv) -> AgentOutcome {
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    for i in 0..20 {
        let task = claim(env);
        let outcome = env.agent.run(&ctx, task).expect("run");
        if !matches!(outcome, AgentOutcome::Continue) {
            return outcome;
        }
        if i == 0 {
            // 第一轮已 spawn sidecar，等待其写出结果。
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    panic!("never reached terminal outcome");
}

#[test]
fn moderate_all_builds_job_and_reports_counts() {
    let env = setup(json!({"mode": "all"}));
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    // 第一轮：spawn sidecar -> Continue，此时 job.json 已写好。
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run 1");
    assert!(matches!(outcome, AgentOutcome::Continue));
    let job: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            env.store
                .root()
                .join("douyin_moderate")
                .join(env.task_id.to_string())
                .join("jobs")
                .join("job.json"),
        )
        .expect("job.json"),
    )
    .expect("parse job");
    // targets = 全部违规作品（发布时间倒序：今天的在前）；不含正常/定时作品。
    let targets: Vec<&str> = job["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .map(|t| t["aweme_id"].as_str().unwrap())
        .collect();
    assert_eq!(targets, vec!["vio_today", "vio_old"]);
    // promoteQueue = 定时作品最晚在前。
    let queue: Vec<&str> = job["promoteQueue"]
        .as_array()
        .expect("queue")
        .iter()
        .map(|t| t["aweme_id"].as_str().unwrap())
        .collect();
    assert_eq!(queue, vec!["sch_late", "sch_early"]);

    let outcome = run_until_terminal(&env);
    match outcome {
        AgentOutcome::Succeeded(v) => {
            assert_eq!(v["privated"], 2);
            assert_eq!(v["published"], 1);
            assert_eq!(v["failed"], 0);
        }
        other => panic!("expected Succeeded, got {other:?}"),
    }
    // 成功后自动入队一轮数据同步刷新作品状态。
    let sync_tasks = TasksRepo::new(&env.db)
        .list(None)
        .expect("tasks")
        .into_iter()
        .filter(|t| t.agent_name == "douyin_sync")
        .count();
    assert_eq!(sync_tasks, 1, "处理成功后应自动入队一轮同步");
}

#[test]
fn moderate_single_targets_only_that_post() {
    let env = setup(json!({"mode": "single", "aweme_id": "vio_old"}));
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run 1");
    assert!(matches!(outcome, AgentOutcome::Continue));
    let job: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            env.store
                .root()
                .join("douyin_moderate")
                .join(env.task_id.to_string())
                .join("jobs")
                .join("job.json"),
        )
        .expect("job.json"),
    )
    .expect("parse job");
    let targets: Vec<&str> = job["targets"]
        .as_array()
        .expect("targets")
        .iter()
        .map(|t| t["aweme_id"].as_str().unwrap())
        .collect();
    assert_eq!(targets, vec!["vio_old"]);
}

#[test]
fn moderate_surfaces_failure() {
    let env = setup(json!({"mode": "all"}));
    let dir = env
        .store
        .root()
        .join("douyin_moderate")
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
