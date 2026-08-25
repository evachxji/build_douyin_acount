use tauri::State;

use crate::domain::error::ApiError;
use crate::state::AppState;
use crate::whisper_models::{self, WhisperModelStatus};

/// 列出全部 Whisper 模型规格的安装状态与下载进度（设置页 1.5s 轮询）。
#[tauri::command]
pub fn list_whisper_models(state: State<AppState>) -> Vec<WhisperModelStatus> {
    whisper_models::list_models(&state.store)
}

/// 后台下载指定模型（route：auto=官方优先镜像兜底 / official / mirror）。
/// 全局同时只允许一个下载；已安装或已有下载进行中时报错。
#[tauri::command]
pub fn start_whisper_download(
    state: State<AppState>,
    model: String,
    route: String,
) -> Result<(), ApiError> {
    whisper_models::start_download(&state.store, &model, &route).map_err(ApiError::from)
}

/// 取消进行中的下载（保留 .part 断点，重新下载可续传）。
#[tauri::command]
pub fn cancel_whisper_download(model: String) -> Result<(), ApiError> {
    whisper_models::cancel_download(&model).map_err(ApiError::from)
}

/// 删除已安装的模型目录（释放磁盘；下载中的模型拒绝删除）。
#[tauri::command]
pub fn delete_whisper_model(state: State<AppState>, model: String) -> Result<(), ApiError> {
    whisper_models::delete_model(&state.store, &model).map_err(ApiError::from)
}
