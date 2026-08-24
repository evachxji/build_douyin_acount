use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::json;

use crate::agents::{Agent, AgentContext, AgentOutcome};
use crate::db::repos::douyin_posts::DouyinPostsRepo;
use crate::db::repos::tasks::TasksRepo;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;
use crate::media::MediaStore;

/// DouyinModerate Agent：处理违规作品——经 opencli 在创作者中心页面内调写接口：
///
/// 1) 违规作品改「仅自己可见」；
/// 2) 若该作品发布日期为今天且还有定时作品，把最晚一条定时作品改为立即发布
///    （权限修改成功后才提升）。
///
/// sidecar（scripts/douyin-sync/moderate.js）执行写操作；本 Agent 从库里装配
/// 处理清单（targets）与可提升的定时队列（promoteQueue），轮询进度，成功后
/// 自动入队一轮数据同步刷新作品状态。
pub struct DouyinModerateAgent {
    node: String,
    script: PathBuf,
}

impl DouyinModerateAgent {
    pub fn new(node: impl Into<String>, script: PathBuf) -> Self {
        Self {
            node: node.into(),
            script,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ModerateState {
    Running,
    Done,
    Cancelled,
    Failed(String),
    Missing,
}

fn read_progress(path: &Path) -> Result<ModerateState, DomainError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ModerateState::Missing),
        Err(e) => return Err(e.into()),
    };
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    Ok(match v["state"].as_str().unwrap_or("") {
        "running" => ModerateState::Running,
        "done" => ModerateState::Done,
        "cancelled" => ModerateState::Cancelled,
        "failed" => ModerateState::Failed(
            v["message"].as_str().unwrap_or("douyin moderate failed").to_string(),
        ),
        other => ModerateState::Failed(format!("unknown douyin moderate state: {other}")),
    })
}

fn mod_dir(store: &MediaStore, task_id: i64) -> PathBuf {
    store.root().join("douyin_moderate").join(task_id.to_string())
}

fn progress_path(store: &MediaStore, task_id: i64) -> PathBuf {
    mod_dir(store, task_id).join("progress.json")
}

fn results_path(store: &MediaStore, task_id: i64) -> PathBuf {
    mod_dir(store, task_id).join("results.jsonl")
}

fn cancel_path(store: &MediaStore, task_id: i64) -> PathBuf {
    store.root().join("douyin_moderate").join("cancel").join(task_id.to_string())
}

/// 请求取消：写 cancel 标记，sidecar 停止后续处理。
pub fn request_cancel(store: &MediaStore, task_id: i64) -> Result<(), DomainError> {
    let path = cancel_path(store, task_id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, "cancel")?;
    Ok(())
}

fn spawn_sidecar(
    ctx: &AgentContext,
    task: &AgentTask,
    self_agent: &DouyinModerateAgent,
) -> Result<(), DomainError> {
    let repo = DouyinPostsRepo::new(ctx.db);
    // 处理清单：单条模式只含目标作品；批量模式含全部违规作品（发布时间倒序）。
    let targets: Vec<serde_json::Value> = if task.payload["mode"].as_str() == Some("single") {
        let aweme_id = task.payload["aweme_id"].as_str().unwrap_or_default();
        let post = repo
            .get_by_aweme(aweme_id)?
            .ok_or_else(|| DomainError::InvalidData(format!("作品不存在: {aweme_id}")))?;
        vec![json!({"aweme_id": post.aweme_id, "create_time": post.create_time})]
    } else {
        repo.list_violating()?
            .iter()
            .map(|p| json!({"aweme_id": p.aweme_id, "create_time": p.create_time}))
            .collect()
    };
    if targets.is_empty() {
        return Err(DomainError::InvalidData("没有需要处理的违规作品".to_string()));
    }
    // 可提升的定时队列：最晚的在前（sidecar 依次 shift）。
    let promote_queue: Vec<serde_json::Value> = repo
        .list_scheduled()?
        .iter()
        .map(|p| json!({"aweme_id": p.aweme_id, "create_time": p.create_time}))
        .collect();

    let dir = mod_dir(&ctx.store, task.id);
    std::fs::create_dir_all(dir.join("jobs"))?;
    let job_path = dir.join("jobs").join("job.json");
    let job = json!({
        "taskId": task.id,
        "progressFile": progress_path(&ctx.store, task.id),
        "resultsFile": results_path(&ctx.store, task.id),
        "cancelFile": cancel_path(&ctx.store, task.id),
        "targets": targets,
        "promoteQueue": promote_queue,
    });
    std::fs::write(&job_path, serde_json::to_string_pretty(&job)?)?;
    // 先写 running 占位再拉起进程：避免下一轮 run_once 在 sidecar 写出进度前重复 spawn。
    std::fs::write(
        progress_path(&ctx.store, task.id),
        serde_json::to_string(&json!({
            "state": "running",
            "done": 0,
            "total": targets.len(),
            "message": "启动违规处理 sidecar",
        }))?,
    )?;
    Command::new(&self_agent.node)
        .arg(&self_agent.script)
        .arg(&job_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| DomainError::Process {
            cmd: self_agent.node.clone(),
            detail: format!("spawn douyin moderate sidecar failed: {e}"),
        })
}

/// 处理失败统一出口：可重试时清空临时目录（下轮经 Missing 重拉 sidecar）。
/// 注意重试语义：sidecar 幂等（已设私密的再设一次无副作用），但「立即发布」
/// 不可回退——失败重试只发生在 sidecar 整体未跑完时（如无心跳/进程退出）。
fn fail_task(store: &MediaStore, task: &AgentTask, msg: String) -> DomainError {
    if task.attempts < task.max_attempts {
        let dir = mod_dir(store, task.id);
        if dir.is_dir() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        DomainError::InvalidData(format!("违规处理失败，将自动重试：{msg}"))
    } else {
        DomainError::InvalidData(format!("违规处理失败：{msg}"))
    }
}

/// 透传 sidecar 进度到任务 payload，供任务面板显示。
fn forward_progress(ctx: &AgentContext, task: &AgentTask, progress_file: &Path) {
    if let Ok(raw) = std::fs::read_to_string(progress_file) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            let _ = TasksRepo::new(ctx.db).update_payload(
                task.id,
                json!({
                    "progress": {
                        "done": v["done"].as_i64().unwrap_or(0),
                        "total": v["total"].as_i64().unwrap_or(0),
                        "message": v["message"].as_str().unwrap_or(""),
                    }
                }),
            );
        }
    }
}

impl Agent for DouyinModerateAgent {
    fn name(&self) -> &'static str {
        "douyin_moderate"
    }

    fn run(&self, ctx: &AgentContext, task: AgentTask) -> Result<AgentOutcome, DomainError> {
        let progress_file = progress_path(&ctx.store, task.id);
        let mut state = read_progress(&progress_file)?;
        if state == ModerateState::Missing {
            spawn_sidecar(ctx, &task, self)?;
            return Ok(AgentOutcome::Continue);
        }
        // 活性检查：sidecar 每 5s 心跳 touch 一次 progress.json；
        // mtime 超过阈值还显示 running，说明进程已被带走（如应用重启），按失败处理走自动重拉。
        if state == ModerateState::Running {
            let stale = std::fs::metadata(&progress_file)
                .and_then(|m| m.modified())
                .map(|t| t.elapsed().unwrap_or_default() > Duration::from_secs(60))
                .unwrap_or(false);
            if stale {
                state = ModerateState::Failed("sidecar 无心跳（进程已退出）".to_string());
            }
        }
        match state {
            ModerateState::Running => {
                forward_progress(ctx, &task, &progress_file);
                Ok(AgentOutcome::Continue)
            }
            ModerateState::Done => {
                // 结果计数以 progress.json 的 stats 为准（清理临时目录前读出）。
                let stats = std::fs::read_to_string(&progress_file)
                    .ok()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
                    .and_then(|v| v.get("stats").cloned())
                    .unwrap_or(serde_json::Value::Null);
                let privated = stats["privated"].as_i64().unwrap_or(0);
                let published = stats["published"].as_i64().unwrap_or(0);
                let failed = stats["failed"].as_i64().unwrap_or(0);
                let dir = mod_dir(&ctx.store, task.id);
                if dir.is_dir() {
                    let _ = std::fs::remove_dir_all(&dir);
                }
                // 作品状态已变（私密/立即发布）：自动入队一轮数据同步刷新日历数据。
                let tasks_repo = TasksRepo::new(ctx.db);
                if !tasks_repo.has_active("douyin_sync")? {
                    tasks_repo.enqueue("douyin_sync", "sync_works", json!({}), false)?;
                }
                Ok(AgentOutcome::Succeeded(json!({
                    "privated": privated,
                    "published": published,
                    "failed": failed,
                })))
            }
            ModerateState::Cancelled => Ok(AgentOutcome::Cancelled(json!({
                "reason": "违规处理已取消（用户请求取消或关闭了浏览器标签页）",
            }))),
            ModerateState::Failed(msg) => Err(fail_task(&ctx.store, &task, msg)),
            ModerateState::Missing => unreachable!(),
        }
    }
}
