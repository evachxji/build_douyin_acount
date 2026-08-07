use std::time::Duration;

use tauri::State;

use crate::domain::error::{ApiError, DomainError};
use crate::state::AppState;

/// 探测本地 Ollama 是否可用（GET /api/version，2s 超时，供顶栏状态标签轮询）。
#[tauri::command]
pub fn check_ollama(state: State<AppState>) -> bool {
    let url = {
        let s = state.settings.read().expect("settings lock poisoned");
        s.ollama_url.clone()
    };
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(_) => return false,
    };
    rt.block_on(async {
        let client = match reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
        {
            Ok(c) => c,
            Err(_) => return false,
        };
        let endpoint = format!("{}/api/version", url.trim_end_matches('/'));
        client
            .get(&endpoint)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    })
}

/// 后台启动本地 Ollama 服务（Windows 下隐藏控制台窗口）。
/// 若已运行，`ollama serve` 会自行退出，不影响现有实例。
#[tauri::command]
pub fn start_ollama() -> Result<(), ApiError> {
    let mut cmd = std::process::Command::new("ollama");
    cmd.arg("serve");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| ApiError::from(DomainError::Process {
            cmd: "ollama".to_string(),
            detail: e.to_string(),
        }))
}
