use std::path::PathBuf;

use tauri::{AppHandle, State};

use crate::backup::{ExportStats, ImportPreview};
use crate::domain::error::{ApiError, DomainError};
use crate::state::AppState;

/// 导出全部用户数据（视频/封面/数据库快照/采集 cookie）为一个 zip 备份包。
#[tauri::command]
pub fn export_backup(state: State<AppState>, dest: String) -> Result<ExportStats, ApiError> {
    let dest = PathBuf::from(&dest);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(DomainError::from)?;
    }
    crate::backup::export_backup(&state.store, &state.db, &dest).map_err(ApiError::from)
}

/// 导入第一步：校验 zip 并解压到暂存区，返回包内统计（不写标记，取消不产生副作用）。
#[tauri::command]
pub fn import_backup(state: State<AppState>, src: String) -> Result<ImportPreview, ApiError> {
    let src = PathBuf::from(&src);
    if !src.is_file() {
        return Err(DomainError::InvalidData(format!("文件不存在: {src:?}")).into());
    }
    crate::backup::stage_import(&state.store, &src).map_err(ApiError::from)
}

/// 导入第二步（用户确认覆盖后）：写 pending 标记，重启时应用。
#[tauri::command]
pub fn confirm_import(state: State<AppState>) -> Result<(), ApiError> {
    crate::backup::mark_import_pending(&state.store).map_err(ApiError::from)
}

/// 重启应用（导入备份后生效用）。
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}
