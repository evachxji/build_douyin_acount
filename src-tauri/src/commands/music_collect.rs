use tauri::State;

use crate::domain::error::ApiError;
use crate::music_collect::{self, CollectProgress};
use crate::state::AppState;

/// 开始后台批量采集剪映音乐库（按分类各采 per_category 首；专用线程执行立即返回，
/// 进度走 music_collect_status 轮询）。已有采集中进行时报错。
#[tauri::command]
pub fn start_music_collect(
    state: State<AppState>,
    per_category: Option<u32>,
) -> Result<(), ApiError> {
    music_collect::start_collect(
        state.db.clone(),
        state.store.clone(),
        per_category.unwrap_or(music_collect::PER_CATEGORY_DEFAULT),
    )
    .map_err(ApiError::from)
}

/// 停止进行中的音乐库采集（已下载的保留，重跑幂等续采）。
#[tauri::command]
pub fn stop_music_collect() -> Result<(), ApiError> {
    music_collect::cancel_collect().map_err(ApiError::from)
}

/// 采集进度（无进度文件为 None；中断的采集读出时判 interrupted）。
#[tauri::command]
pub fn music_collect_status(state: State<AppState>) -> Option<CollectProgress> {
    music_collect::collect_status(&state.store)
}
