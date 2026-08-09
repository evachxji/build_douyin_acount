use serde_json::json;

use crate::db::repos::assets::AssetsRepo;
use crate::db::Database;
use crate::db::repos::tasks::TasksRepo;
use crate::domain::error::{ApiError, DomainError};
use crate::state::AppState;

use tauri::State;

/// 批量入队视觉分析任务；已有 PENDING/RUNNING 任务的资产跳过（避免重复分析）。
/// 返回实际入队的资产 id。
pub fn enqueue_analysis(db: &Database, asset_ids: &[i64]) -> Result<Vec<i64>, DomainError> {
    let mut enqueued = Vec::new();
    for asset_id in asset_ids {
        // 锁作用域要收窄：查询后立即释放，避免与 enqueue 内部的加锁死锁。
        let active: bool = {
            let conn = db.conn();
            conn.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM agent_tasks
                    WHERE agent_name = 'vision'
                      AND status IN ('PENDING','RUNNING')
                      AND json_extract(payload_json, '$.asset_id') = ?1)",
                [asset_id],
                |r| r.get(0),
            )?
        };
        if active {
            continue;
        }
        // 新的分析意图：清除旧的「手工淘汰」标记，使已淘汰素材可经 AI 分析重新评估。
        let _ = AssetsRepo::new(db).set_eliminate_reason(*asset_id, None);
        TasksRepo::new(db).enqueue(
            "vision",
            "analyze",
            json!({ "asset_id": asset_id }),
            false,
        )?;
        enqueued.push(*asset_id);
    }
    Ok(enqueued)
}

/// GUI 入口：批量入队分析任务，返回实际入队的资产 id。
#[tauri::command]
pub fn analyze_assets(
    state: State<AppState>,
    ids: Vec<i64>,
) -> Result<Vec<i64>, ApiError> {
    enqueue_analysis(&state.db, &ids).map_err(ApiError::from)
}
