//! Publisher Agent 集成测试：用假 sidecar（tests/fixtures/fake_publish.cjs）验证
//! 人工选文件 -> 自动填表 -> 人工确认发布 的完整人机协作状态流。
use std::path::{Path, PathBuf};

use build_douyin_acount_lib::agents::publisher::{request_confirm, request_continue};
use build_douyin_acount_lib::agents::{Agent, AgentContext, AgentOutcome};
use build_douyin_acount_lib::agents::publisher::PublisherAgent;
use build_douyin_acount_lib::db::repos::assets::AssetsRepo;
use build_douyin_acount_lib::db::repos::drafts::DraftsRepo;
use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::domain::asset::{AssetStatus, NewAsset};
use build_douyin_acount_lib::domain::draft::{DraftStatus, NewDraft};
use build_douyin_acount_lib::domain::task::{AgentTask, TaskStatus};
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
        .update_status(asset_id, AssetStatus::Ready)
        .expect("ready");
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

    let agent = PublisherAgent::new("node", fixture("fake_publish.cjs"));
    TestEnv {
        _dir: dir,
        db,
        store,
        agent,
        asset_id,
        draft_id,
    }
}

fn task(env: &TestEnv) -> AgentTask {
    AgentTask {
        id: 1,
        agent_name: "publisher".to_string(),
        task_type: "prepare_publish".to_string(),
        payload: json!({ "draft_id": env.draft_id, "asset_id": env.asset_id }),
        status: TaskStatus::Running,
        attempts: 1,
        max_attempts: 3,
        next_retry_at: None,
        human_required: true,
        error: None,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
    }
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
fn publisher_waits_for_human_file_selection_with_path() {
    let env = setup();
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let outcome = env.agent.run(&ctx, task(&env)).expect("run");
    let payload = match outcome {
        AgentOutcome::WaitingForHuman(p) => p,
        other => panic!("expected WaitingForHuman, got {other:?}"),
    };
    assert_eq!(payload["stage"], "file");
    assert_eq!(payload["path"], "videos/test.mp4");
}

#[test]
fn publisher_continues_after_file_selected_and_reaches_form_ready() {
    let env = setup();
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    // 第一次运行：等待人工选文件
    let first = env.agent.run(&ctx, task(&env)).expect("run");
    assert!(matches!(first, AgentOutcome::WaitingForHuman(_)));

    // 用户点【已选择】 -> 写 continue 标记 -> 再次运行：轮到上传完成并进入 form_ready
    request_continue(&env.store, env.draft_id).expect("continue");
    let second = env.agent.run(&ctx, task(&env)).expect("run 2");
    let payload = match second {
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
    let outcome = env.agent.run(&ctx, task(&env)).expect("run");
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
    let outcome = env.agent.run(&ctx, task(&env)).expect("run");
    assert!(matches!(outcome, AgentOutcome::Succeeded(_)));
    let asset = AssetsRepo::new(&env.db).get(env.asset_id).expect("get").expect("asset");
    assert_eq!(asset.status, AssetStatus::Published);
}

#[test]
fn publisher_fails_when_browser_closed_before_publish() {
    let env = setup();
    write_progress(&env, "browser_closed", "window closed");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let err = env.agent.run(&ctx, task(&env)).expect_err("must fail");
    assert!(err.to_string().contains("浏览器标签页已关闭"), "{err}");
    let asset = AssetsRepo::new(&env.db).get(env.asset_id).expect("get").expect("asset");
    assert_eq!(asset.status, AssetStatus::Selected);
}

#[test]
fn publisher_surfaces_sidecar_failure() {
    let env = setup();
    write_progress(&env, "failed", "selector not found");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let err = env.agent.run(&ctx, task(&env)).expect_err("must fail");
    assert!(err.to_string().contains("selector not found"), "{err}");
}

#[test]
fn clipboard_write_roundtrip() {
    let text = format!("video-path-test-{}", std::process::id());
    build_douyin_acount_lib::commands::publish::copy_to_clipboard(text.clone())
        .expect("copy to clipboard");
    let mut clipboard = arboard::Clipboard::new().expect("open clipboard");
    assert_eq!(clipboard.get_text().expect("read clipboard"), text);
}
