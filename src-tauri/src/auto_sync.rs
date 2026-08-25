//! 自动巡检调度：开关开启时每 30 分钟发起一轮抖音数据同步（douyin_sync），
//! 同步完成发现违规作品时自动入队违规处理（douyin_moderate，现成逻辑）。
//!
//! 调度器本体在 lib.rs setup 的后台线程里，每 60s 调一次 `maybe_enqueue`。
//! 联动发生在 DouyinSyncAgent 完成分支（agents/douyin_sync.rs）：只有
//! payload 带 `"source": "auto"` 的轮次才会触发自动违规处理——手动点击的
//! 同步永不触发，moderate 完成后自动入队的刷新 sync（无 source）也不会
//! 再触发，链路不会成环。

use serde_json::json;

use crate::db::repos::tasks::TasksRepo;
use crate::db::Database;
use crate::domain::error::DomainError;
use crate::settings::SettingsRepo;

/// 两轮自动同步的最小间隔。
pub const INTERVAL_SECS: i64 = 30 * 60;

/// 开关开启且距上次同步任务创建已超过 30 分钟时，入队一轮 `douyin_sync`
/// （payload 标记 `"source": "auto"` 供完成分支联动违规处理），返回 true。
/// 已有 sync / moderate 任务在进行或排队时不重复发起；moderate 完成后会
/// 自行入队一轮刷新 sync，无需调度器代劳。
pub fn maybe_enqueue(db: &Database) -> Result<bool, DomainError> {
    if !SettingsRepo::new(db).get_auto_sync_moderate()? {
        return Ok(false);
    }
    let tasks = TasksRepo::new(db);
    if tasks.has_active("douyin_sync")? || tasks.has_active("douyin_moderate")? {
        return Ok(false);
    }
    // created_at 是 SQLite `datetime('now')`（UTC），直接用 SQL 比较间隔。
    let recent: bool = db.conn().query_row(
        "SELECT EXISTS(
             SELECT 1 FROM agent_tasks
             WHERE agent_name = 'douyin_sync'
               AND created_at >= datetime('now', ?1))",
        [format!("-{} seconds", INTERVAL_SECS)],
        |r| r.get(0),
    )?;
    if recent {
        return Ok(false);
    }
    tasks.enqueue("douyin_sync", "sync_works", json!({ "source": "auto" }), false)?;
    Ok(true)
}
