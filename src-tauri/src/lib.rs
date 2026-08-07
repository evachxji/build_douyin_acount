pub mod analysis;
pub mod agents;
pub mod commands;
pub mod db;
pub mod domain;
pub mod media;
pub mod settings;
pub mod state;
pub mod util;

use std::path::PathBuf;

use crate::db::Database;
use crate::media::MediaStore;
use crate::settings::SettingsRepo;
use crate::state::AppState;
use tauri::Manager;

/// 定位本地存储根目录：开发期取仓库根下的 storage/，否则取可执行文件旁。
fn storage_root() -> PathBuf {
    if let Ok(dir) = std::env::current_dir() {
        if dir.join("frontend").is_dir() {
            return dir.join("storage");
        }
    }
    // `cargo run` 的工作目录是 src-tauri，此时回退到仓库根（仅开发构建）。
    #[cfg(debug_assertions)]
    {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Some(parent) = manifest.parent() {
            if parent.join("frontend").is_dir() {
                return parent.join("storage");
            }
        }
    }
    std::env::current_exe()
        .map(|p| {
            p.parent()
                .map(|d| d.join("storage"))
                .unwrap_or_else(|| PathBuf::from("storage"))
        })
        .unwrap_or_else(|_| PathBuf::from("storage"))
}

/// 应用信息命令：供前端冒烟测试与后续版本展示使用。
#[tauri::command]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "name": "build_douyin_acount",
        "phase": 3,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let store = MediaStore::new(storage_root());
    store.ensure_dirs().expect("create storage dirs");
    let db = Database::open(&store.db_dir().join("app.db")).expect("open database");
    let settings = SettingsRepo::new(&db).load().expect("load settings");
    let state = AppState::new(db, store, settings);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            app_info,
            commands::assets::list_assets,
            commands::assets::get_asset,
            commands::assets::get_analysis,
            commands::assets::delete_asset,
            commands::assets::update_asset_status,
            commands::assets::import_videos,
            commands::assets::reveal_in_folder,
            commands::assets::library_overview,
            commands::analysis::analyze_assets,
            commands::collection::start_collection,
            commands::content::generate_copy,
            commands::content::get_copy,
            commands::content::get_recommendations,
            commands::publish::prepare_publish,
            commands::publish::list_drafts,
            commands::publish::publish_confirmed,
            commands::publish::copy_to_clipboard,
            commands::tasks::list_tasks,
            commands::tasks::get_task,
            commands::tasks::cancel_task,
            commands::tasks::resume_task,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::ollama::check_ollama,
            commands::ollama::start_ollama,
        ])
        .setup(|app| {
            // 后台 worker：串行消费 vision 任务队列（含失败退避重试）。
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let state = handle.state::<AppState>();
                if let Ok(runtime) = state.build_runtime() {
                    if let Err(e) = runtime.run_once("vision") {
                        tracing::warn!("vision worker error: {e}");
                    }
                }
            });
            // 后台 worker：publisher / collector / content 任务（独立线程，避免阻塞 vision 队列）。
            let handle_pub = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let state = handle_pub.state::<AppState>();
                if let Ok(runtime) = state.build_runtime() {
                    if let Err(e) = runtime.run_once("publisher") {
                        tracing::warn!("publisher worker error: {e}");
                    }
                    if let Err(e) = runtime.run_once("collector") {
                        tracing::warn!("collector worker error: {e}");
                    }
                    if let Err(e) = runtime.run_once("content") {
                        tracing::warn!("content worker error: {e}");
                    }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
