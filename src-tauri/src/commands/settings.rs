use tauri::State;

use crate::domain::error::ApiError;
use crate::settings::{AppSettings, SettingsRepo};
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> AppSettings {
    state.settings.read().expect("settings lock poisoned").clone()
}

#[tauri::command]
pub fn update_settings(
    state: State<AppState>,
    mut settings: AppSettings,
) -> Result<AppSettings, ApiError> {
    // 保留数据库中已有的条件判定依据（LLM 分类结果），避免前端陈旧副本覆盖清空。
    let existing = SettingsRepo::new(&state.db).load().map_err(ApiError::from)?;
    for (name, basis) in existing.condition_basis {
        settings.condition_basis.entry(name).or_insert(basis);
    }
    SettingsRepo::new(&state.db).save(&settings).map_err(ApiError::from)?;
    *state.settings.write().expect("settings lock poisoned") = settings.clone();
    Ok(settings)
}
