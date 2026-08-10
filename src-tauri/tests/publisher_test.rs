//! Publisher Agent 集成测试：用假 sidecar 验证「自动上传文件 -> 自动填表 ->
//! 人工确认发布」的完整人机协作状态流。
use std::path::{Path, PathBuf};

use build_douyin_acount_lib::agents::publisher::{request_confirm, PublisherAgent};
use build_douyin_acount_lib::agents::{Agent, AgentContext, AgentOutcome};
use build_douyin_acount_lib::db::repos::assets::AssetsRepo;
use build_douyin_acount_lib::db::repos::drafts::DraftsRepo;
use build_douyin_acount_lib::db::repos::tasks::TasksRepo;
use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::domain::asset::{AssetStatus, NewAsset};
use build_douyin_acount_lib::domain::draft::{DraftStatus, NewDraft};
use build_douyin_acount_lib::domain::task::TaskStatus;
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
    agent: PublisherAgent,
    asset_id: i64,
    draft_id: i64,
    task_id: i64,
}

fn setup() -> TestEnv {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(&dir.path().join("test.db")).expect("db");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("dirs");

    let assets = AssetsRepo::new(&db);
    let asset_id = assets
        .insert(NewAsset {
            dedup_key: Some("publisher-test".to_string()),
            file_path: "videos/test.mp4".to_string(),
            cover_path: None,
            source_platform: "instagram".to_string(),
            source_url: None,
            author: Some("tester".to_string()),
            title: Some("测试视频".to_string()),
            description: None,
            published_at: None,
            tags: vec!["猫".to_string()],
            likes: 0,
            comments: 0,
            plays: 0,
            duration_ms: 5000,
        })
        .expect("insert asset");
    assets
        .update_status(asset_id, AssetStatus::Analyzing)
        .expect("analyzing");
    assets
        .update_status(asset_id, AssetStatus::Candidate)
        .expect("CANDIDATE");
    assets
        .update_status(asset_id, AssetStatus::Selected)
        .expect("selected");

    let draft_id = DraftsRepo::new(&db)
        .create(NewDraft {
            asset_id,
            style_id: None,
            title: Some("测试标题".to_string()),
            body: Some("测试文案".to_string()),
            hashtags: vec!["萌宠".to_string()],
        })
        .expect("create draft");

    let task_id = TasksRepo::new(&db)
        .enqueue(
            "publisher",
            "prepare_publish",
            json!({ "draft_id": draft_id, "asset_id": asset_id }),
            true,
        )
        .expect("enqueue");
    let agent = PublisherAgent::new("node", fixture("fake_publish.cjs"));
    TestEnv {
        _dir: dir,
        db,
        store,
        agent,
        asset_id,
        draft_id,
        task_id,
    }
}

/// 领取任务并返回 RUNNING 状态的任务（模拟 worker run_once）。
fn claim(env: &TestEnv) -> build_douyin_acount_lib::domain::task::AgentTask {
    TasksRepo::new(&env.db)
        .claim_next("publisher")
        .expect("claim")
        .expect("some task")
}

fn progress_path(env: &TestEnv) -> PathBuf {
    env.store
        .root()
        .join("publish")
        .join("progress")
        .join(format!("{}.json", env.draft_id))
}

fn write_progress(env: &TestEnv, state: &str, message: &str) {
    let path = progress_path(env);
    std::fs::create_dir_all(path.parent().unwrap()).expect("dirs");
    std::fs::write(
        path,
        serde_json::to_string(&json!({ "state": state, "message": message })).unwrap(),
    )
    .expect("write progress");
}

#[test]
fn publisher_file_stage_keeps_task_running_with_info_payload() {
    let env = setup();
    // 预写 waiting_for_file（自动上传失败的兜底态）：验证任务保持 RUNNING 且 payload 写入阶段信息。
    write_progress(&env, "waiting_for_file", "waiting");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Continue));
    let t = TasksRepo::new(&env.db).get(env.task_id).expect("get").expect("task");
    assert_eq!(t.status, TaskStatus::Running);
    assert_eq!(t.payload["stage"], "file");
    assert_eq!(t.payload["title"], "测试标题");
    assert_eq!(t.payload["path"], "videos/test.mp4");
    // sidecar 的兜底提示消息透传到 payload，供前端弹窗展示。
    assert_eq!(t.payload["message"], "waiting");
}

#[test]
fn publisher_spawns_sidecar_and_auto_fills() {
    let env = setup();
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    // 模拟 worker 每轮重新领取：sidecar 拉起后自动完成 uploading -> form_filled，
    // agent 在后续轮次看到 form_filled 进入 form_ready。
    let mut outcome = None;
    for _ in 0..20 {
        let task = claim(&env);
        outcome = Some(env.agent.run(&ctx, task).expect("run"));
        if !matches!(outcome.as_ref().unwrap(), AgentOutcome::Continue) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let outcome = outcome.expect("ran at least once");
    let payload = match outcome {
        AgentOutcome::WaitingForHuman(p) => p,
        other => panic!("expected WaitingForHuman, got {other:?}"),
    };
    assert_eq!(payload["stage"], "form_ready");
    let draft = DraftsRepo::new(&env.db).get(env.draft_id).expect("get").expect("draft");
    assert_eq!(draft.status, DraftStatus::Filled);
}

#[test]
fn publisher_auto_detects_form_ready_after_fill() {
    let env = setup();
    // 假 sidecar 已自动完成填表
    write_progress(&env, "form_filled", "fake form filled");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    let payload = match outcome {
        AgentOutcome::WaitingForHuman(p) => p,
        other => panic!("expected WaitingForHuman, got {other:?}"),
    };
    assert_eq!(payload["stage"], "form_ready");
    let draft = DraftsRepo::new(&env.db).get(env.draft_id).expect("get").expect("draft");
    assert_eq!(draft.status, DraftStatus::Filled);
}

#[test]
fn publisher_finalizes_when_human_confirms_publish() {
    let env = setup();
    write_progress(&env, "form_filled", "filled");
    request_confirm(&env.store, env.draft_id).expect("confirm");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Succeeded(_)));
    let draft = DraftsRepo::new(&env.db).get(env.draft_id).expect("get").expect("draft");
    assert_eq!(draft.status, DraftStatus::Published);
    let asset = AssetsRepo::new(&env.db).get(env.asset_id).expect("get").expect("asset");
    assert_eq!(asset.status, AssetStatus::Published);
}

#[test]
fn publisher_finalizes_when_sidecar_detects_success() {
    let env = setup();
    write_progress(&env, "published", "detected publish toast");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Succeeded(_)));
    let asset = AssetsRepo::new(&env.db).get(env.asset_id).expect("get").expect("asset");
    assert_eq!(asset.status, AssetStatus::Published);
}

#[test]
fn publisher_cancels_when_browser_closed_before_publish() {
    // 用户关闭标签页 = 取消（终态），不再按失败重试。
    let env = setup();
    write_progress(&env, "browser_closed", "window closed");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Cancelled(_)), "{outcome:?}");
}

#[test]
fn publisher_cancels_when_sidecar_cancelled() {
    let env = setup();
    write_progress(&env, "cancelled", "用户取消");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Cancelled(_)), "{outcome:?}");
}

#[test]
fn publisher_surfaces_sidecar_failure() {
    let env = setup();
    write_progress(&env, "failed", "selector not found");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let err = env.agent.run(&ctx, task).expect_err("must fail");
    assert!(err.to_string().contains("selector not found"), "{err}");
}

#[test]
fn publisher_respawns_when_sidecar_heartbeat_stale() {
    // 进度冻结在 uploading（模拟 sidecar 随应用重启被杀的僵尸任务）；
    // 阈值归零 => 立即判定僵尸，应重拉 sidecar 并在 payload 计数。
    let env = setup();
    write_progress(&env, "uploading", "stuck mid-upload");
    let agent = PublisherAgent::new("node", fixture("fake_publish.cjs"))
        .with_stale_after(std::time::Duration::ZERO);
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Continue), "{outcome:?}");
    let t = TasksRepo::new(&env.db).get(env.task_id).expect("get").expect("task");
    assert_eq!(t.payload["respawns"], 1, "僵尸判定后应重拉 sidecar 并计数");
}

#[test]
fn publisher_stale_sidecar_cancelled_after_max_respawns() {
    // 已连续重拉 2 次仍无心跳 => 取消任务（终态），提示用户重新点击【准备发布】。
    let env = setup();
    write_progress(&env, "uploading", "stuck");
    TasksRepo::new(&env.db)
        .update_payload(env.task_id, json!({ "respawns": 2 }))
        .expect("seed respawns");
    let agent = PublisherAgent::new("node", fixture("fake_publish.cjs"))
        .with_stale_after(std::time::Duration::ZERO);
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = agent.run(&ctx, task).expect("run");
    match outcome {
        AgentOutcome::Cancelled(v) => {
            assert!(v["reason"].as_str().unwrap_or("").contains("无响应"), "{v}")
        }
        other => panic!("expected Cancelled, got {other:?}"),
    }
}

#[test]
fn publisher_never_respawns_form_filled_even_when_stale() {
    // form_filled 是人工等待态（确认/取消走标记文件），sidecar 死了也不需要重拉。
    let env = setup();
    write_progress(&env, "form_filled", "filled");
    let agent = PublisherAgent::new("node", fixture("fake_publish.cjs"))
        .with_stale_after(std::time::Duration::ZERO);
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::WaitingForHuman(_)), "{outcome:?}");
    let t = TasksRepo::new(&env.db).get(env.task_id).expect("get").expect("task");
    assert!(t.payload.get("respawns").is_none(), "form_filled 不得计重拉次数");
}

#[test]
fn clipboard_write_roundtrip() {
    let text = format!("video-path-test-{}", std::process::id());
    build_douyin_acount_lib::commands::publish::copy_to_clipboard(text.clone())
        .expect("copy to clipboard");
    let mut clipboard = arboard::Clipboard::new().expect("open clipboard");
    assert_eq!(clipboard.get_text().expect("read clipboard"), text);
}
