use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{Manager, State};

use crate::db::Database;
use crate::domain::error::{ApiError, DomainError};
use crate::settings::SettingsRepo;
use crate::state::AppState;
use crate::tools::ffmpeg;

#[derive(Debug, Serialize)]
pub struct FfmpegStatus {
    pub installed: bool,
    pub current_version: Option<String>,
    pub exe_path: String,
}

#[derive(Debug, Serialize)]
pub struct FfmpegUpdateInfo {
    pub latest_version: String,
    pub update_available: bool,
}

#[derive(Debug, Serialize)]
pub struct CachedFfmpegUpdate {
    /// checking / ok / failed
    pub state: String,
    pub info: Option<FfmpegUpdateInfo>,
}

/// 更新进度（前端 1s 轮询；单文件下载，内存态即可——同 yt-dlp 契约）。
#[derive(Debug, Clone, Serialize)]
pub struct FfmpegUpdateProgress {
    /// downloading / verifying / done / error（verifying 含 zip 解压与 -version 校验）
    pub stage: String,
    pub downloaded: u64,
    pub total: u64,
    pub speed_bps: u64,
    /// done：新版本号或「已是最新」；error：错误详情。
    pub message: String,
}

static UPDATE_PROGRESS: Mutex<Option<FfmpegUpdateProgress>> = Mutex::new(None);

/// 每日更新检查缓存（内存态；db 的 `ffmpeg_update_check` key 是跨进程持久层——
/// 同一天重启 app 直接回填，不重复联网）。
#[derive(Debug, Clone)]
enum UpdateCheckCache {
    Checking,
    Ok(String),
    Failed,
}

static UPDATE_CHECK: Mutex<Option<UpdateCheckCache>> = Mutex::new(None);

fn set_update_check(c: UpdateCheckCache) {
    *UPDATE_CHECK.lock().expect("ffmpeg update check lock poisoned") = Some(c);
}

/// 每天首次启动时由 lib.rs 后台线程调用：db 里已有今日记录 → 回填缓存不联网；
/// 否则联网查 gyan.dev 最新版，结果落 db + 内存缓存（失败也记录，当天不再自动重试）。
pub fn maybe_auto_check(db: &Database) {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let repo = SettingsRepo::new(db);
    if let Ok(Some((date, latest))) = repo.get_ffmpeg_update_check() {
        if date == today {
            set_update_check(match latest {
                Some(v) => UpdateCheckCache::Ok(v),
                None => UpdateCheckCache::Failed,
            });
            return;
        }
    }
    set_update_check(UpdateCheckCache::Checking);
    let result =
        ffmpeg::http_client(Duration::from_secs(10)).and_then(|c| block_on(ffmpeg::fetch_latest_version(&c))?);
    match result {
        Ok(latest) => {
            let _ = repo.set_ffmpeg_update_check(&today, Some(&latest));
            set_update_check(UpdateCheckCache::Ok(latest));
        }
        Err(e) => {
            tracing::warn!("FFmpeg 每日更新检查失败（可到设置页手动重试）: {e}");
            let _ = repo.set_ffmpeg_update_check(&today, None);
            set_update_check(UpdateCheckCache::Failed);
        }
    }
}

fn status(store: &crate::media::MediaStore) -> FfmpegStatus {
    let exe = ffmpeg::exe_path(store);
    let installed = exe.exists();
    FfmpegStatus {
        installed,
        current_version: if installed {
            // exe 未变化时走缓存（spawn 冷启动只做一次）。
            ffmpeg::current_version_cached(&exe)
        } else {
            None
        },
        exe_path: exe.to_string_lossy().into_owned(),
    }
}

fn set_progress(stage: &str, downloaded: u64, total: u64, speed_bps: u64, message: String) {
    *UPDATE_PROGRESS
        .lock()
        .expect("ffmpeg update progress lock poisoned") = Some(FfmpegUpdateProgress {
        stage: stage.to_string(),
        downloaded,
        total,
        speed_bps,
        message,
    });
}

fn is_active(p: &Option<FfmpegUpdateProgress>) -> bool {
    matches!(
        p.as_ref().map(|p| p.stage.as_str()),
        Some("downloading") | Some("verifying")
    )
}

/// 内置 FFmpeg 本地状态（设置页打开时调用）。冷缓存需 spawn `ffmpeg.exe -version`，
/// 故走 spawn_blocking 不占用主线程。
#[tauri::command]
pub async fn get_ffmpeg_status(app: tauri::AppHandle) -> FfmpegStatus {
    let store = app.state::<AppState>().store.clone();
    tauri::async_runtime::spawn_blocking(move || status(&store))
        .await
        .unwrap_or_else(|_| FfmpegStatus {
            installed: false,
            current_version: None,
            exe_path: String::new(),
        })
}

/// 查询 gyan.dev 最新版本并比对当前版本（联网；手动「检查更新」按钮用。
/// 成功后刷新内存缓存与 db——当天设置页读到的就是这份）。
#[tauri::command]
pub fn check_ffmpeg_update(state: State<AppState>) -> Result<FfmpegUpdateInfo, ApiError> {
    let current = status(&state.store).current_version;
    let client = ffmpeg::http_client(Duration::from_secs(10)).map_err(ApiError::from)?;
    let latest = block_on(ffmpeg::fetch_latest_version(&client))?.map_err(ApiError::from)?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let _ = SettingsRepo::new(&state.db).set_ffmpeg_update_check(&today, Some(&latest));
    set_update_check(UpdateCheckCache::Ok(latest.clone()));
    Ok(FfmpegUpdateInfo {
        // 未安装时一律提示可更新（按钮即「下载安装」）。
        update_available: current
            .as_deref()
            .map(|c| ffmpeg::update_available(c, &latest))
            .unwrap_or(true),
        latest_version: latest,
    })
}

/// 设置页读取每日检查缓存（**不联网**——每天只由启动时的后台静默检查填充；
/// checking = 启动检查在途，failed = 当天检查失败，可点「检查更新」手动重试）。
#[tauri::command]
pub async fn get_cached_ffmpeg_update(app: tauri::AppHandle) -> CachedFfmpegUpdate {
    let cache = UPDATE_CHECK
        .lock()
        .expect("ffmpeg update check lock poisoned")
        .clone();
    match cache {
        None | Some(UpdateCheckCache::Checking) => CachedFfmpegUpdate {
            state: "checking".to_string(),
            info: None,
        },
        Some(UpdateCheckCache::Failed) => CachedFfmpegUpdate {
            state: "failed".to_string(),
            info: None,
        },
        Some(UpdateCheckCache::Ok(latest)) => {
            let store = app.state::<AppState>().store.clone();
            let current = tauri::async_runtime::spawn_blocking(move || {
                status(&store).current_version
            })
            .await
            .unwrap_or(None);
            CachedFfmpegUpdate {
                state: "ok".to_string(),
                info: Some(FfmpegUpdateInfo {
                    // update_available 读时现算：一键更新后 current 变了而 latest 缓存仍有效。
                    update_available: current
                        .as_deref()
                        .map(|c| ffmpeg::update_available(c, &latest))
                        .unwrap_or(true),
                    latest_version: latest,
                }),
            }
        }
    }
}

/// 一键更新（未安装时即下载安装）：**后台线程**执行，立即返回；
/// 前端经 `get_ffmpeg_update_progress` 轮询进度。已在下载/校验中重复调用是幂等空操作。
/// 流程：查最新版（已是最新则短路不下载）→ 流式下载 zip（回报进度）→ 解压双 exe 并验证 → rename 替换。
#[tauri::command]
pub fn update_ffmpeg(state: State<AppState>) -> Result<(), ApiError> {
    {
        let p = UPDATE_PROGRESS.lock().expect("ffmpeg update progress lock poisoned");
        if is_active(&p) {
            return Ok(());
        }
    }
    let store = state.store.clone();
    std::thread::spawn(move || run_update(store));
    Ok(())
}

/// 更新进度查询（无更新进行时为 None）。
#[tauri::command]
pub fn get_ffmpeg_update_progress() -> Option<FfmpegUpdateProgress> {
    UPDATE_PROGRESS
        .lock()
        .expect("ffmpeg update progress lock poisoned")
        .clone()
}

fn run_update(store: crate::media::MediaStore) {
    let exe = ffmpeg::exe_path(&store);
    let tmp_zip = exe.with_extension("zip.new");
    // 短路：已是最新就不下载（~106MB 的 zip 不能白下；查不到最新版则不短路，照常走下载）。
    let up_to_date = (|| -> Option<String> {
        let client = ffmpeg::http_client(Duration::from_secs(10)).ok()?;
        let latest = block_on(ffmpeg::fetch_latest_version(&client)).ok()?.ok()?;
        let cur = ffmpeg::current_version(&exe)?;
        if ffmpeg::update_available(&cur, &latest) {
            None
        } else {
            Some(latest)
        }
    })();
    if let Some(latest) = up_to_date {
        set_progress("done", 0, 0, 0, format!("已是最新版本 {latest}"));
        return;
    }
    let result: Result<(), DomainError> = (|| {
        let dl_client = ffmpeg::http_client(Duration::from_secs(600))?;
        let start = Instant::now();
        set_progress("downloading", 0, 0, 0, String::new());
        block_on(ffmpeg::download_latest(&dl_client, &tmp_zip, |downloaded, total| {
            let speed = (downloaded as f64 / start.elapsed().as_secs_f64().max(0.5)) as u64;
            set_progress("downloading", downloaded, total, speed, String::new());
        }))??;
        set_progress("verifying", 0, 0, 0, String::new());
        // install 内完成解压 + 逐个 -version 校验 + rename 轮换。
        ffmpeg::install(&store, &tmp_zip)
    })();
    match result {
        Ok(()) => {
            let _ = std::fs::remove_file(&tmp_zip);
            let version = ffmpeg::current_version(&exe).unwrap_or_else(|| "最新版本".to_string());
            set_progress("done", 0, 0, 0, format!("已更新至 {version}"));
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_zip);
            set_progress("error", 0, 0, 0, e.to_string());
        }
    }
}

/// 与 commands/ollama.rs 同一模式：同步 command 内起一次性 runtime 跑 reqwest。
fn block_on<F: std::future::Future>(future: F) -> Result<F::Output, DomainError> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| DomainError::Process {
        cmd: "tokio".to_string(),
        detail: e.to_string(),
    })?;
    Ok(rt.block_on(future))
}
