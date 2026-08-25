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
    let repo = TasksRepo::new(&state.db);
    let task = repo.get(id).map_err(ApiError::from)?;
    repo.cancel(id).map_err(ApiError::from)?;
    // publisher 任务取消时同步通知 sidecar 关闭浏览器窗口。
    if let Some(t) = task {
        if t.agent_name == "publisher" {
            if let Some(draft_id) = t.payload["draft_id"].as_i64() {
                crate::agents::publisher::request_cancel(&state.store, draft_id)
                    .map_err(ApiError::from)?;
            }
        }
        if t.agent_name == "collector" {
            crate::agents::collector::request_cancel(&state.store, t.id)
                .map_err(ApiError::from)?;
        }
        if t.agent_name == "douyin_sync" {
            crate::agents::douyin_sync::request_cancel(&state.store, t.id)
                .map_err(ApiError::from)?;
        }
        if t.agent_name == "douyin_moderate" {
            crate::agents::douyin_moderate::request_cancel(&state.store, t.id)
                .map_err(ApiError::from)?;
        }
        if t.agent_name == "douyin_export" {
            crate::agents::douyin_export::request_cancel(&state.store, t.id)
                .map_err(ApiError::from)?;
        }
    }
    Ok(())
}

/// 人工确认：放行任务继续执行（WAITING_FOR_HUMAN -> RUNNING）。
#[tauri::command]
pub fn resume_task(state: State<AppState>, id: i64) -> Result<(), ApiError> {
    TasksRepo::new(&state.db).resume(id).map_err(ApiError::from)
}
