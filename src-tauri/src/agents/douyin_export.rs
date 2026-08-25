//! DouyinExport Agent：从抖音创作者平台拉取全部已发布（含定时待发布）作品的
//! 详细数据，导出为 Excel 清单。
//!
//! 复用 douyin_sync 的 sidecar（sync.js）拉取最新数据入库（封面/完播率/跳出率/
//! 违规等全字段），完成后把 douyin_posts 中 published + scheduled 的作品写成
//! Excel（含封面缩略图），返回文件路径供前端弹窗下载。

use std::path::PathBuf;
use std::time::Duration;

use serde_json::json;

use crate::agents::douyin_sync::{self, SyncState};
use crate::agents::{Agent, AgentContext, AgentOutcome};
use crate::db::repos::douyin_posts::DouyinPostsRepo;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;
use crate::media::MediaStore;

pub struct DouyinExportAgent {
    node: String,
    script: PathBuf,
}

impl DouyinExportAgent {
    pub fn new(node: impl Into<String>, script: PathBuf) -> Self {
        Self {
            node: node.into(),
            script,
        }
    }
}

/// 请求取消：与同步共用同一套 cancel 标记机制（sidecar 停止翻页）。
pub fn request_cancel(store: &MediaStore, task_id: i64) -> Result<(), DomainError> {
    douyin_sync::request_cancel(store, task_id)
}

fn export_dir(store: &MediaStore, task_id: i64) -> PathBuf {
    store.root().join("exports").join(task_id.to_string())
}

/// 导出失败统一出口：清空同步与导出临时目录，按剩余尝试次数区分消息。
fn fail_task(store: &MediaStore, task: &AgentTask, msg: String) -> DomainError {
    if task.attempts < task.max_attempts {
        for dir in [
            douyin_sync::sync_dir(store, task.id),
            export_dir(store, task.id),
        ] {
            if dir.is_dir() {
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
        DomainError::InvalidData(format!("导出失败，将自动重试：{msg}"))
    } else {
        DomainError::InvalidData(format!("导出失败：{msg}"))
    }
}

impl Agent for DouyinExportAgent {
    fn name(&self) -> &'static str {
        "douyin_export"
    }

    fn run(&self, ctx: &AgentContext, task: AgentTask) -> Result<AgentOutcome, DomainError> {
        let progress_file = douyin_sync::progress_path(&ctx.store, task.id);
        let mut state = douyin_sync::read_progress(&progress_file)?;
        if state == SyncState::Missing {
            douyin_sync::spawn_sidecar(ctx, &task, &self.node, &self.script)?;
            return Ok(AgentOutcome::Continue);
        }
        // 活性检查：sidecar 每 5s 心跳 touch progress.json；超时视为进程已被带走。
        if state == SyncState::Running {
            let stale = std::fs::metadata(&progress_file)
                .and_then(|m| m.modified())
                .map(|t| t.elapsed().unwrap_or_default() > Duration::from_secs(60))
                .unwrap_or(false);
            if stale {
                state = SyncState::Failed("sidecar 无心跳（进程已退出）".to_string());
            }
        }
        match state {
            SyncState::Running => {
                // 增量入库 + 透传进度，与同步一致。
                if let Err(e) = douyin_sync::import_new_posts(ctx, task.id) {
                    let _ = crate::db::repos::tasks::TasksRepo::new(ctx.db)
                        .add_log(task.id, "WARN", &format!("增量入库失败（下轮重试）：{e}"));
                }
                if let Ok(raw) = std::fs::read_to_string(&progress_file) {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                        let _ = crate::db::repos::tasks::TasksRepo::new(ctx.db).update_payload(
                            task.id,
                            json!({
                                "progress": {
                                    "synced": v["synced"].as_i64().unwrap_or(0),
                                    "scheduled": v["scheduled"].as_i64().unwrap_or(0),
                                    "message": v["message"].as_str().unwrap_or(""),
                                }
                            }),
                        );
                    }
                }
                Ok(AgentOutcome::Continue)
            }
            SyncState::Done => {
                let (synced, scheduled, _) = douyin_sync::import_new_posts(ctx, task.id)?;
                let posts = DouyinPostsRepo::new(ctx.db).list_for_export()?;
                let dir = export_dir(&ctx.store, task.id);
                std::fs::create_dir_all(&dir)?;
                let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                let file = dir.join(format!("douyin_export_{stamp}.xlsx"));
                let stats = crate::export::write_douyin_excel(&posts, &file)?;
                // 数据已入库并导出，清理同步临时目录。
                let sync_dir = douyin_sync::sync_dir(&ctx.store, task.id);
                if sync_dir.is_dir() {
                    let _ = std::fs::remove_dir_all(&sync_dir);
                }
                Ok(AgentOutcome::Succeeded(json!({
                    "synced": synced,
                    "scheduled": scheduled,
                    "rows": stats.rows,
                    "images": stats.images,
                    "file_path": stats.file_path,
                })))
            }
            SyncState::Cancelled => Ok(AgentOutcome::Cancelled(json!({
                "reason": "导出已取消（用户请求取消或关闭了浏览器标签页）",
            }))),
            SyncState::Failed(msg) => Err(fail_task(&ctx.store, &task, msg)),
            SyncState::Missing => unreachable!(),
        }
    }
}
