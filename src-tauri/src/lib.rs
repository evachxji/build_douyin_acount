pub mod analysis;
pub mod agents;
pub mod auto_sync;
pub mod backup;
pub mod commands;
pub mod db;
pub mod domain;
pub mod media;
pub mod music_mix;
pub mod music_sync;
pub mod music_covers;
pub mod settings;
pub mod state;
pub mod tools;
pub mod util;
pub mod whisper_models;

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
    // 安装版：exe 位于 Program Files，普通用户无写权限，落到用户本地数据目录。
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        if !local.is_empty() {
            return PathBuf::from(local).join("build_douyin_acount").join("storage");
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

/// 显示并聚焦主窗口（托盘左键/「显示窗口」菜单/二次启动时调用）。
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let store = MediaStore::new(storage_root());
    store.ensure_dirs().expect("create storage dirs");
    // 覆盖式导入的第二段：重启后、打开数据库之前应用暂存内容（此时无文件句柄，替换安全）；
    // 失败已回滚到原数据，记错误日志后继续用现有数据启动。
    if let Err(e) = backup::apply_pending_import(&store) {
        tracing::error!("apply pending import failed: {e}");
    }
    let db = Database::open(&store.db_dir().join("app.db")).expect("open database");
    let settings = SettingsRepo::new(&db).load().expect("load settings");
    let state = AppState::new(db, store, settings);

    tauri::Builder::default()
        // 单实例必须最先注册：托盘常驻后用户可能再次双击图标——拦截二次启动，
        // 改为显示已有窗口（双开会争用 SQLite 与 opencli 浏览器会话）。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            app_info,
            commands::assets::list_assets,
            commands::assets::get_asset,
            commands::assets::get_analysis,
            commands::assets::delete_asset,
            commands::assets::update_asset_status,
            commands::assets::reveal_in_folder,
            commands::assets::library_overview,
            commands::analysis::analyze_assets,
            commands::backup::export_backup,
            commands::backup::import_backup,
            commands::backup::confirm_import,
            commands::backup::restart_app,
            commands::collection::start_collection,
            commands::douyin::start_douyin_sync,
            commands::douyin::list_douyin_posts,
            commands::douyin::get_auto_sync_moderate,
            commands::douyin::set_auto_sync_moderate,
            commands::douyin::moderate_douyin_violation,
            commands::douyin::moderate_all_violations,
            commands::douyin::open_url,
            commands::music::get_music_status,
            commands::music::sync_music,
            commands::music::open_music_dir,
            commands::music::list_music_files,
            commands::music::add_music_to_video,
            commands::music::ensure_music_covers,
            commands::content::generate_copy,
            commands::content::get_copy,
            commands::content::update_copy,
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
            commands::whisper_models::list_whisper_models,
            commands::whisper_models::start_whisper_download,
            commands::whisper_models::cancel_whisper_download,
            commands::whisper_models::delete_whisper_model,
            commands::ytdlp::get_ytdlp_status,
            commands::ytdlp::check_ytdlp_update,
            commands::ytdlp::update_ytdlp,
            commands::ytdlp::get_ytdlp_update_progress,
            commands::ytdlp::get_cached_ytdlp_update,
            commands::ffmpeg::get_ffmpeg_status,
            commands::ffmpeg::check_ffmpeg_update,
            commands::ffmpeg::update_ffmpeg,
            commands::ffmpeg::get_ffmpeg_update_progress,
            commands::ffmpeg::get_cached_ffmpeg_update,
        ])
        .setup(|app| {
            // 内置 yt-dlp：安装版首启从打包资源释放到数据目录（仅缺失时释放，
            // 绝不覆盖——用户一键更新过的新版不能被内置旧版顶掉）。
            {
                let mut candidates: Vec<PathBuf> = app
                    .path()
                    .resource_dir()
                    .map(|d| vec![d.join("yt-dlp.exe")])
                    .unwrap_or_default();
                candidates.push(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/yt-dlp.exe"),
                );
                let state = app.handle().state::<AppState>();
                tools::ytdlp::ensure_from_bundle(&state.store, &candidates);
            }
            // 内置 FFmpeg：安装版首启从打包资源释放 ffmpeg/ffprobe 到数据目录
            //（仅缺失时释放，绝不覆盖）。随后把 tools 目录登记给 run_tool——
            // 抽帧/封面/时长探测/音乐合成全部优先用内置 exe，FFMPEG_PATH 环境变量仍可覆盖。
            {
                let mut candidates: Vec<PathBuf> = app
                    .path()
                    .resource_dir()
                    .map(|d| vec![d])
                    .unwrap_or_default();
                candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"));
                let state = app.handle().state::<AppState>();
                tools::ffmpeg::ensure_from_bundle(&state.store, &candidates);
                crate::util::set_tools_dir(state.store.tools_dir());
            }
            // 每天首次启动后台静默检查一次 FFmpeg 更新（同 yt-dlp 契约）。
            let handle_ff = app.handle().clone();
            std::thread::spawn(move || {
                let state = handle_ff.state::<AppState>();
                commands::ffmpeg::maybe_auto_check(&state.db);
            });
            // 每天首次启动后台静默检查一次 yt-dlp 更新（结果缓存内存+落 db；
            // 设置页只读缓存不联网，「检查更新」按钮是手动强制刷新）。
            let handle_yd = app.handle().clone();
            std::thread::spawn(move || {
                let state = handle_yd.state::<AppState>();
                commands::ytdlp::maybe_auto_check(&state.db);
            });
            // 系统托盘：关窗仅隐藏到托盘，后台任务（分析/发布/巡检）继续运行；
            // 仅托盘菜单【退出】真正结束进程。左键单击托盘图标恢复窗口。
            let show = tauri::menu::MenuItemBuilder::with_id("show", "显示窗口").build(app)?;
            let quit = tauri::menu::MenuItemBuilder::with_id("quit", "退出").build(app)?;
            let menu = tauri::menu::Menu::with_items(app, &[&show, &quit])?;
            tauri::tray::TrayIconBuilder::with_id("main")
                .icon(tauri::image::Image::from_bytes(include_bytes!("../icons/icon.ico"))?)
                .tooltip("抖音素材运营助手")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;
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
            // 后台 worker：publisher / collector / douyin_sync / content 任务（独立线程，避免阻塞 vision 队列）。
            // publisher 走 run_active 并行模型：每轮驱动全部活跃发布任务（sidecar 会话按
            // draft_id 隔离，定时时间在 spawn 时并发占位顺延）；其余 Agent 维持串行。
            let handle_pub = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let state = handle_pub.state::<AppState>();
                if let Ok(runtime) = state.build_runtime() {
                    if let Err(e) = runtime.run_active("publisher") {
                        tracing::warn!("publisher worker error: {e}");
                    }
                    if let Err(e) = runtime.run_once("collector") {
                        tracing::warn!("collector worker error: {e}");
                    }
                    if let Err(e) = runtime.run_once("douyin_sync") {
                        tracing::warn!("douyin_sync worker error: {e}");
                    }
                    if let Err(e) = runtime.run_once("douyin_moderate") {
                        tracing::warn!("douyin_moderate worker error: {e}");
                    }
                    if let Err(e) = runtime.run_once("content") {
                        tracing::warn!("content worker error: {e}");
                    }
                }
            });
            // 自动巡检调度：每 60s 检查一次；开关开且距上次同步 ≥30 分钟则
            // 入队 douyin_sync（payload 带 source=auto，完成分支联动违规处理）。
            let handle_auto = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                let state = handle_auto.state::<AppState>();
                if let Err(e) = auto_sync::maybe_enqueue(&state.db) {
                    tracing::warn!("auto sync scheduler error: {e}");
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关窗不退出：拦截关闭请求仅隐藏（托盘仍在，后台任务继续运行）。
            // 真正退出走托盘菜单【退出】（app.exit）。
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
