use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;

use crate::agents::{Agent, AgentContext, AgentOutcome};
use crate::db::repos::assets::AssetsRepo;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;
use crate::media::{ImportOptions, MediaStore};

/// Collector Agent：刷平台推荐流采集短视频（TikTok / Instagram Reels）。
///
/// sidecar（opencli + yt-dlp）负责发现与下载；本 Agent 轮询进度，
/// 完成后把下载的视频导入素材库并自动入队 AI 分析。
/// 配额（条数/比例/点赞上限）由任务 payload 携带（来自设置，发起时固化）。
pub struct CollectorAgent {
    node: String,
    script: PathBuf,
    ytdlp: String,
}

impl CollectorAgent {
    pub fn new(node: impl Into<String>, script: PathBuf, ytdlp: impl Into<String>) -> Self {
        Self {
            node: node.into(),
            script,
            ytdlp: ytdlp.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CollectorState {
    Running,
    Done,
    Cancelled,
    Failed(String),
    Missing,
}

fn read_progress(path: &Path) -> Result<CollectorState, DomainError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(CollectorState::Missing),
        Err(e) => return Err(e.into()),
    };
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    Ok(match v["state"].as_str().unwrap_or("") {
        "running" => CollectorState::Running,
        "done" => CollectorState::Done,
        "cancelled" => CollectorState::Cancelled,
        "failed" => CollectorState::Failed(
            v["message"].as_str().unwrap_or("collector failed").to_string(),
        ),
        other => CollectorState::Failed(format!("unknown collector state: {other}")),
    })
}

fn collect_dir(store: &MediaStore, task_id: i64) -> PathBuf {
    store.root().join("collect").join(task_id.to_string())
}

fn progress_path(store: &MediaStore, task_id: i64) -> PathBuf {
    collect_dir(store, task_id).join("progress.json")
}

fn videos_path(store: &MediaStore, task_id: i64) -> PathBuf {
    collect_dir(store, task_id).join("videos.jsonl")
}

fn output_dir(store: &MediaStore, task_id: i64) -> PathBuf {
    collect_dir(store, task_id).join("output")
}

fn cancel_path(store: &MediaStore, task_id: i64) -> PathBuf {
    store.root().join("collect").join("cancel").join(task_id.to_string())
}

/// 请求取消：写 cancel 标记，sidecar 停止下载。
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
    self_agent: &CollectorAgent,
) -> Result<(), DomainError> {
    let dir = collect_dir(&ctx.store, task.id);
    std::fs::create_dir_all(dir.join("jobs"))?;
    let job_path = dir.join("jobs").join("job.json");
    let job = json!({
        "platform": task.payload["platform"].as_str().unwrap_or("tiktok"),
        "count": task.payload["count"].as_i64().unwrap_or(10),
        "maxLikes": task.payload["maxLikes"].as_i64().unwrap_or(1_000_000),
        "taskId": task.id,
        "outputDir": output_dir(&ctx.store, task.id),
        "progressFile": progress_path(&ctx.store, task.id),
        "videosFile": videos_path(&ctx.store, task.id),
        "cancelFile": cancel_path(&ctx.store, task.id),
        "ytdlpPath": self_agent.ytdlp,
    });
    std::fs::write(&job_path, serde_json::to_string_pretty(&job)?)?;
    Command::new(&self_agent.node)
        .arg(&self_agent.script)
        .arg(&job_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| DomainError::Process {
            cmd: self_agent.node.clone(),
            detail: format!("spawn collector sidecar failed: {e}"),
        })
}

/// 导入 sidecar 下载的视频并回填互动数据；返回导入的素材 id。
fn import_videos(ctx: &AgentContext, task_id: i64) -> Result<Vec<i64>, DomainError> {
    let vp = videos_path(&ctx.store, task_id);
    let raw = match std::fs::read_to_string(&vp) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let assets = AssetsRepo::new(ctx.db);
    let manager = crate::media::VideoManager::new(ctx.db, ctx.store.clone());
    let mut imported = Vec::new();
    for line in raw.lines().filter(|l| !l.trim().is_empty()) {
        let v: serde_json::Value = serde_json::from_str(line)?;
        let file = v["file"].as_str().unwrap_or_default();
        let platform = v["platform"].as_str().unwrap_or("local");
        let id = v["id"].as_str().unwrap_or_default();
        if file.is_empty() || !Path::new(file).is_file() {
            continue;
        }
        let asset = manager.import_file(
            Path::new(file),
            ImportOptions {
                source_platform: platform.to_string(),
                title: v["title"].as_str().map(str::to_string),
                source_url: v["url"].as_str().map(str::to_string),
                author: v["author"].as_str().map(str::to_string),
                dedup_key: Some(format!("{platform}:{id}")),
            },
        )?;
        assets.update_stats(
            asset.id,
            v["likes"].as_i64().unwrap_or(0),
            v["comments"].as_i64().unwrap_or(0),
            v["plays"].as_i64().unwrap_or(0),
        )?;
        imported.push(asset.id);
    }
    Ok(imported)
}

impl Agent for CollectorAgent {
    fn name(&self) -> &'static str {
        "collector"
    }

    fn run(&self, ctx: &AgentContext, task: AgentTask) -> Result<AgentOutcome, DomainError> {
        let progress_file = progress_path(&ctx.store, task.id);
        let state = read_progress(&progress_file)?;
        if state == CollectorState::Missing {
            spawn_sidecar(ctx, &task, self)?;
            return Ok(AgentOutcome::Continue);
        }
        match state {
            CollectorState::Running => Ok(AgentOutcome::Continue),
            CollectorState::Done => {
                let imported = import_videos(ctx, task.id)?;
                // 自动入队 AI 分析
                let enqueued = crate::commands::analysis::enqueue_analysis(ctx.db, &imported)?;
                // 视频已复制进素材库，清理采集临时目录。
                let dir = collect_dir(&ctx.store, task.id);
                if dir.is_dir() {
                    let _ = std::fs::remove_dir_all(&dir);
                }
                Ok(AgentOutcome::Succeeded(json!({
                    "imported": imported.len(),
                    "enqueued_analysis": enqueued.len(),
                })))
            }
            CollectorState::Cancelled => Err(DomainError::InvalidData("采集已取消。".to_string())),
            CollectorState::Failed(msg) => {
                Err(DomainError::InvalidData(format!("采集失败：{msg}")))
            }
            CollectorState::Missing => unreachable!(),
        }
    }
}
