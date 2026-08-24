use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::json;

use crate::agents::{Agent, AgentContext, AgentOutcome};
use crate::db::repos::douyin_posts::DouyinPostsRepo;
use crate::domain::douyin::NewDouyinPost;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;
use crate::media::MediaStore;

/// DouyinSync Agent：经 opencli 桥接已登录 Chrome，页面内 fetch 创作者中心
/// 作品列表 API 分页同步（标题/封面/播放/点赞/评论/发布时间，含定时发布）。
///
/// sidecar（scripts/douyin-sync/sync.js）负责拉取与封面下载；本 Agent 轮询进度，
/// 把 posts.jsonl 逐行幂等 upsert 进 douyin_posts 表（播放数等统计每次全量刷新）。
pub struct DouyinSyncAgent {
    node: String,
    script: PathBuf,
}

impl DouyinSyncAgent {
    pub fn new(node: impl Into<String>, script: PathBuf) -> Self {
        Self {
            node: node.into(),
            script,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SyncState {
    Running,
    Done,
    Cancelled,
    Failed(String),
    Missing,
}

fn read_progress(path: &Path) -> Result<SyncState, DomainError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SyncState::Missing),
        Err(e) => return Err(e.into()),
    };
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    Ok(match v["state"].as_str().unwrap_or("") {
        "running" => SyncState::Running,
        "done" => SyncState::Done,
        "cancelled" => SyncState::Cancelled,
        "failed" => SyncState::Failed(
            v["message"].as_str().unwrap_or("douyin sync failed").to_string(),
        ),
        other => SyncState::Failed(format!("unknown douyin sync state: {other}")),
    })
}

fn sync_dir(store: &MediaStore, task_id: i64) -> PathBuf {
    store.root().join("douyin_sync").join(task_id.to_string())
}

fn progress_path(store: &MediaStore, task_id: i64) -> PathBuf {
    sync_dir(store, task_id).join("progress.json")
}

fn posts_path(store: &MediaStore, task_id: i64) -> PathBuf {
    sync_dir(store, task_id).join("posts.jsonl")
}

fn cancel_path(store: &MediaStore, task_id: i64) -> PathBuf {
    store.root().join("douyin_sync").join("cancel").join(task_id.to_string())
}

/// 请求取消：写 cancel 标记，sidecar 停止翻页。
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
    self_agent: &DouyinSyncAgent,
) -> Result<(), DomainError> {
    let dir = sync_dir(&ctx.store, task.id);
    std::fs::create_dir_all(dir.join("jobs"))?;
    let job_path = dir.join("jobs").join("job.json");
    // 已有违规原因（brief）的作品跳过详情补抓（违规原因不会变，省一串详情请求）；
    // 违规状态本身仍以本轮列表为准，库里已消解的旧违规会被 upsert 清掉。
    let known_violations =
        DouyinPostsRepo::new(ctx.db).list_violation_brief_ids()?;
    let job = json!({
        "taskId": task.id,
        "progressFile": progress_path(&ctx.store, task.id),
        "postsFile": posts_path(&ctx.store, task.id),
        "cancelFile": cancel_path(&ctx.store, task.id),
        "coverDir": ctx.store.covers_dir(),
        "maxPages": 50,
        "knownViolations": known_violations,
    });
    std::fs::write(&job_path, serde_json::to_string_pretty(&job)?)?;
    // 先写 running 占位再拉起进程：避免下一轮 run_once 在 sidecar 写出进度前重复 spawn。
    std::fs::write(
        progress_path(&ctx.store, task.id),
        serde_json::to_string(&json!({
            "state": "running",
            "synced": 0,
            "message": "启动同步 sidecar",
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
            detail: format!("spawn douyin sync sidecar failed: {e}"),
        })
}

/// 同步失败统一出口：可重试时清空临时目录（下轮经 Missing 重拉 sidecar），
/// 按剩余尝试次数区分消息。
fn fail_task(store: &MediaStore, task: &AgentTask, msg: String) -> DomainError {
    if task.attempts < task.max_attempts {
        let dir = sync_dir(store, task.id);
        if dir.is_dir() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        DomainError::InvalidData(format!("同步失败，将自动重试：{msg}"))
    } else {
        DomainError::InvalidData(format!("同步失败：{msg}"))
    }
}

/// 把 sidecar 已写出的 posts.jsonl 逐行 upsert 进 douyin_posts（幂等，整文件重读
/// 也只会刷新统计）；返回（总行数，定时数，违规数）。
fn import_new_posts(ctx: &AgentContext, task_id: i64) -> Result<(i64, i64, i64), DomainError> {
    let pp = posts_path(&ctx.store, task_id);
    let raw = match std::fs::read_to_string(&pp) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((0, 0, 0)),
        Err(e) => return Err(e.into()),
    };
    let repo = DouyinPostsRepo::new(ctx.db);
    let mut total = 0i64;
    let mut scheduled = 0i64;
    let mut violations = 0i64;
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let post: NewDouyinPost = match serde_json::from_str(line) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("douyin sync: skip malformed posts.jsonl line: {e}");
                continue;
            }
        };
        repo.upsert(&post)?;
        total += 1;
        if post.status == "scheduled" {
            scheduled += 1;
        }
        if post.violation_text.is_some() {
            violations += 1;
        }
    }
    Ok((total, scheduled, violations))
}

impl Agent for DouyinSyncAgent {
    fn name(&self) -> &'static str {
        "douyin_sync"
    }

    fn run(&self, ctx: &AgentContext, task: AgentTask) -> Result<AgentOutcome, DomainError> {
        let progress_file = progress_path(&ctx.store, task.id);
        let mut state = read_progress(&progress_file)?;
        if state == SyncState::Missing {
            spawn_sidecar(ctx, &task, self)?;
            return Ok(AgentOutcome::Continue);
        }
        // 活性检查：sidecar 每 5s 心跳 touch 一次 progress.json；
        // mtime 超过阈值还显示 running，说明进程已被带走（如应用重启），按失败处理走自动重拉。
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
                if let Err(e) = import_new_posts(ctx, task.id) {
                    let _ = crate::db::repos::tasks::TasksRepo::new(ctx.db)
                        .add_log(task.id, "WARN", &format!("增量入库失败（下轮重试）：{e}"));
                }
                // 透传 sidecar 进度到任务 payload，供任务面板显示当前进度。
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
                // 累计统计以 posts.jsonl 全量行为准（Running 阶段已增量入库过大部分行）。
                let (synced, scheduled, violations) = import_new_posts(ctx, task.id)?;
                // 数据已全部入库，清理同步临时目录。
                let dir = sync_dir(&ctx.store, task.id);
                if dir.is_dir() {
                    let _ = std::fs::remove_dir_all(&dir);
                }
                Ok(AgentOutcome::Succeeded(json!({
                    "synced": synced,
                    "scheduled": scheduled,
                    "violations": violations,
                })))
            }
            SyncState::Cancelled => Ok(AgentOutcome::Cancelled(json!({
                "reason": "同步已取消（用户请求取消或关闭了浏览器标签页）",
            }))),
            SyncState::Failed(msg) => {
                // 不在此处重拉 sidecar：清空临时目录后返回 Err 消耗一次尝试，
                // 由 runtime 退避后重新认领、经 Missing 分支重拉。
                Err(fail_task(&ctx.store, &task, msg))
            }
            SyncState::Missing => unreachable!(),
        }
    }
}
