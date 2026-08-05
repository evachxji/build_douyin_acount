use tauri::State;

use crate::db::repos::tasks::TasksRepo;
use crate::domain::error::ApiError;
use crate::domain::task::{AgentTask, TaskStatus};
use crate::state::AppState;

#[tauri::command]
pub fn list_tasks(
    state: State<AppState>,
    status: Option<String>,
) -> Result<Vec<AgentTask>, ApiError> {
    let status = status
        .as_deref()
        .map(str::parse::<TaskStatus>)
        .transpose()
        .map_err(ApiError::from)?;
    TasksRepo::new(&state.db).list(status).map_err(ApiError::from)
}

#[tauri::command]
pub fn get_task(
    state: State<AppState>,
    id: i64,
) -> Result<serde_json::Value, ApiError> {
    let repo = TasksRepo::new(&state.db);
    let task: Option<AgentTask> = repo.get(id).map_err(ApiError::from)?;
    let logs = match &task {
        Some(t) => repo.logs(t.id).map_err(ApiError::from)?,
        None => Vec::new(),
    };
    Ok(serde_json::json!({ "task": task, "logs": logs }))
}

/// 人工确认：驳回任务（WAITING_FOR_HUMAN -> CANCELLED）。
#[tauri::command]
pub fn cancel_task(state: State<AppState>, id: i64) -> Result<(), ApiError> {
    TasksRepo::new(&state.db).cancel(id).map_err(ApiError::from)
}

/// 人工确认：放行任务继续执行（WAITING_FOR_HUMAN -> RUNNING）。
#[tauri::command]
pub fn resume_task(state: State<AppState>, id: i64) -> Result<(), ApiError> {
    TasksRepo::new(&state.db).resume(id).map_err(ApiError::from)
}
