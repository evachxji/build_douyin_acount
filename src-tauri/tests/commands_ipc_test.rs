//! 通过 Tauri mock runtime 直接走 IPC 验证命令层：
//! 参数名（camelCase）、错误映射、状态机约束。

use build_douyin_acount_lib::commands;
use build_douyin_acount_lib::db::repos::assets::AssetsRepo;
use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::media::MediaStore;
use build_douyin_acount_lib::settings::AppSettings;
use build_douyin_acount_lib::state::AppState;
use build_douyin_acount_lib::domain::asset::NewAsset;
use serde_json::json;
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{ipc::CallbackFn, ipc::InvokeBody, WebviewWindow};

fn sample_asset(i: i64) -> NewAsset {
    NewAsset {
        dedup_key: Some(format!("ipc:{i}")),
        file_path: format!("videos/{i}.mp4"),
        cover_path: None,
        source_platform: "instagram".to_string(),
        source_url: None,
        author: Some("author".to_string()),
        title: Some(format!("视频 {i}")),
        description: None,
        published_at: None,
        tags: Vec::new(),
        likes: 0,
        comments: 0,
        plays: 0,
        duration_ms: 5000,
    }
}

fn invoke(
    webview: &WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    let res = get_ipc_response(
        webview,
        InvokeRequest {
            cmd: cmd.to_string(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    );
    match res {
        // 命令成功：反序列化响应体
        Ok(body) => body
            .deserialize::<serde_json::Value>()
            .map_err(|e| json!({ "serialize_error": e.to_string() })),
        // 命令返回 Err：错误码与 message 已在 JSON 值中
        Err(e) => Err(e),
    }
}

fn build_app(db: Database, store: MediaStore) -> tauri::App<tauri::test::MockRuntime> {
    let state = AppState::new(db, store, AppSettings::default());
    mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::assets::list_assets,
            commands::assets::library_overview,
            commands::assets::update_asset_status,
            commands::analysis::analyze_assets,
            commands::settings::get_settings,
            commands::tasks::list_tasks,
        ])
        .build(mock_context(noop_assets()))
        .expect("build app")
}

#[test]
fn commands_work_over_ipc_with_frontend_arg_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(&dir.path().join("test.db")).expect("db");
    let repo = AssetsRepo::new(&db);
    let a = repo.insert(sample_asset(1)).expect("insert a");
    let b = repo.insert(sample_asset(2)).expect("insert b");
    let store = MediaStore::new(dir.path().join("storage"));
    let app = build_app(db, store);
    let webview =
        tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
            .build()
            .expect("webview");

    // list_assets：空过滤返回全部
    let listed = invoke(&webview, "list_assets", json!({ "filter": {} }))
        .expect("list_assets ok");
    assert_eq!(listed.as_array().map(Vec::len), Some(2));

    // library_overview：总数与未分类计数
    let overview = invoke(&webview, "library_overview", json!({})).expect("overview ok");
    assert_eq!(overview["total"], json!(2));
    assert!(overview["categories"]
        .as_array()
        .expect("categories")
        .iter()
        .any(|c| c["name"] == "未分类" && c["count"] == 2));

    // get_settings：默认模型配置
    let settings = invoke(&webview, "get_settings", json!({})).expect("settings ok");
    assert_eq!(settings["vision_model"], json!("qwen2.5vl:7b"));

    // 非法状态迁移 -> 明确错误码
    let err = invoke(
        &webview,
        "update_asset_status",
        json!({ "id": b, "to": "PUBLISHED" }),
    )
    .expect_err("illegal transition must fail");
    assert_eq!(err["code"], json!("ILLEGAL_TRANSITION"));

    // analyze_assets：批量入队，重复调用不重复入队
    let enqueued = invoke(&webview, "analyze_assets", json!({ "ids": [a, b] }))
        .expect("enqueue ok");
    assert_eq!(enqueued.as_array().map(Vec::len), Some(2));
    let again = invoke(&webview, "analyze_assets", json!({ "ids": [a, b] }))
        .expect("enqueue again ok");
    assert_eq!(again.as_array().map(Vec::len), Some(0), "active tasks must dedupe");

    // list_tasks：能看到 vision 任务
    let tasks = invoke(&webview, "list_tasks", json!({ "status": null }))
        .expect("tasks ok");
    assert_eq!(tasks.as_array().map(Vec::len), Some(2));
    assert!(tasks
        .as_array()
        .expect("tasks array")
        .iter()
        .all(|t| t["agent_name"] == "vision"));
}
