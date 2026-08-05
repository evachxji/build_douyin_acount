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
    settings: AppSettings,
) -> Result<AppSettings, ApiError> {
    SettingsRepo::new(&state.db).save(&settings).map_err(ApiError::from)?;
    *state.settings.write().expect("settings lock poisoned") = settings.clone();
    Ok(settings)
}
