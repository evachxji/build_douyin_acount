use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use serde_json::json;

use crate::agents::{Agent, AgentContext, AgentOutcome};
use crate::db::repos::assets::AssetsRepo;
use crate::db::repos::drafts::DraftsRepo;
use crate::db::repos::tasks::TasksRepo;
use crate::domain::asset::AssetStatus;
use crate::domain::draft::DraftStatus;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;

/// Publisher Agent：辅助发布到 creator.douyin.com（人机协作）。
///
/// 流程：
/// 1. sidecar 在用户已登录的 Chrome 中新开上传页标签，等待用户手动选择视频文件；
/// 2. 用户选好文件后点应用内【已选择】（写入 continue 标记），sidecar 自动填写
///    标题/文案/话题并停在【发布】按钮前（WAITING_FOR_HUMAN）；
/// 3. 用户核对后在浏览器中本人点击发布，再点应用内【我已发布】（写入 confirm 标记），
///    本 Agent 据此标记资产 PUBLISHED。
///
/// 【安全红线】本 Agent 与 sidecar 永不点击最终发布按钮；发布完成状态只由
/// 显式「发布成功」提示或用户本人确认产生。
pub struct PublisherAgent {
    node: String,
    script: PathBuf,
}

impl PublisherAgent {
    pub fn new(node: impl Into<String>, script: PathBuf) -> Self {
        Self {
            node: node.into(),
            script,
        }
    }
}

/// sidecar 进度文件状态。
#[derive(Debug, Clone, PartialEq, Eq)]
enum SidecarState {
    BrowserStarted,
    WaitingForFile,
    Uploading,
    FormFilled,
    Published,
    BrowserClosed,
    Cancelled,
    Failed(String),
    Missing,
}

fn read_progress(path: &Path) -> Result<SidecarState, DomainError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SidecarState::Missing),
        Err(e) => return Err(e.into()),
    };
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    Ok(match v["state"].as_str().unwrap_or("") {
        "browser_started" => SidecarState::BrowserStarted,
        "waiting_for_file" => SidecarState::WaitingForFile,
        "uploading" => SidecarState::Uploading,
        "form_filled" => SidecarState::FormFilled,
        "published" => SidecarState::Published,
        "browser_closed" => SidecarState::BrowserClosed,
        "cancelled" => SidecarState::Cancelled,
        "failed" => SidecarState::Failed(
            v["message"].as_str().unwrap_or("sidecar failed").to_string(),
        ),
        other => SidecarState::Failed(format!("unknown sidecar state: {other}")),
    })
}

fn publish_dir(store: &crate::media::MediaStore) -> PathBuf {
    store.root().join("publish")
}

fn progress_path(store: &crate::media::MediaStore, draft_id: i64) -> PathBuf {
    publish_dir(store)
        .join("progress")
        .join(format!("{draft_id}.json"))
}

fn marker_path(store: &crate::media::MediaStore, dir: &str, draft_id: i64) -> PathBuf {
    publish_dir(store).join(dir).join(format!("{draft_id}"))
}

fn cancel_path(store: &crate::media::MediaStore, draft_id: i64) -> PathBuf {
    marker_path(store, "cancel", draft_id)
}

fn confirm_path(store: &crate::media::MediaStore, draft_id: i64) -> PathBuf {
    marker_path(store, "confirm", draft_id)
}

fn write_marker(store: &crate::media::MediaStore, dir: &str, draft_id: i64) -> Result<(), DomainError> {
    let path = marker_path(store, dir, draft_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, "1")?;
    Ok(())
}

/// 请求取消：写 cancel 标记，sidecar 关闭标签页。
pub fn request_cancel(store: &crate::media::MediaStore, draft_id: i64) -> Result<(), DomainError> {
    write_marker(store, "cancel", draft_id)
}

/// 用户确认已在浏览器中本人点击发布：写 confirm 标记，Agent 据此收尾。
pub fn request_confirm(store: &crate::media::MediaStore, draft_id: i64) -> Result<(), DomainError> {
    write_marker(store, "confirm", draft_id)
}

fn spawn_sidecar(
    ctx: &AgentContext,
    draft_id: i64,
    asset: &crate::domain::asset::VideoAsset,
    draft: &crate::domain::draft::PublishDraft,
    self_agent: &PublisherAgent,
) -> Result<(), DomainError> {
    let base = publish_dir(&ctx.store);
    let jobs = base.join("jobs");
    std::fs::create_dir_all(&jobs)?;
    let job_path = jobs.join(format!("{draft_id}.json"));
    let job = json!({
        "draftId": draft_id,
        "videoPath": asset.file_path,
        "title": draft.title,
        "body": draft.body,
        "hashtags": draft.hashtags,
        "progressFile": progress_path(&ctx.store, draft_id),
        "cancelFile": cancel_path(&ctx.store, draft_id),
        "confirmFile": confirm_path(&ctx.store, draft_id),
    });
    std::fs::write(&job_path, serde_json::to_string_pretty(&job)?)?;

    // 独立子进程；stdout/stderr 落空（进度经文件上报），失败时由轮询发现。
    Command::new(&self_agent.node)
        .arg(&self_agent.script)
        .arg(&job_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| DomainError::Process {
            cmd: self_agent.node.clone(),
            detail: format!("spawn sidecar failed: {e}"),
        })
}

fn finalize_published(ctx: &AgentContext, draft_id: i64, asset_id: i64) -> Result<(), DomainError> {
    DraftsRepo::new(ctx.db).update_status(draft_id, DraftStatus::Published)?;
    AssetsRepo::new(ctx.db).update_status(asset_id, AssetStatus::Published)?;
    Ok(())
}

fn waiting_json(
    stage: &str,
    draft_id: i64,
    asset_id: i64,
    path: Option<&str>,
    message: &str,
) -> AgentOutcome {
    let mut payload = json!({
        "stage": stage,
        "draft_id": draft_id,
        "asset_id": asset_id,
        "message": message,
    });
    if let Some(p) = path {
        payload["path"] = json!(p);
    }
    AgentOutcome::WaitingForHuman(payload)
}

impl Agent for PublisherAgent {
    fn name(&self) -> &'static str {
        "publisher"
    }

    fn run(&self, ctx: &AgentContext, task: AgentTask) -> Result<AgentOutcome, DomainError> {
        let draft_id = task.payload["draft_id"]
            .as_i64()
            .ok_or_else(|| DomainError::InvalidData("payload missing draft_id".to_string()))?;
        let asset_id = task.payload["asset_id"]
            .as_i64()
            .ok_or_else(|| DomainError::InvalidData("payload missing asset_id".to_string()))?;

        let draft = DraftsRepo::new(ctx.db)
            .get(draft_id)?
            .ok_or_else(|| DomainError::InvalidData(format!("draft {draft_id} not found")))?;
        let asset = AssetsRepo::new(ctx.db)
            .get(asset_id)?
            .ok_or(DomainError::AssetNotFound(asset_id))?;

        let progress_file = progress_path(&ctx.store, draft_id);
        let mut state = read_progress(&progress_file)?;

        // 首次执行：sidecar 尚未启动则拉起。
        if state == SidecarState::Missing {
            spawn_sidecar(ctx, draft_id, &asset, &draft, self)?;
            state = SidecarState::BrowserStarted;
        }

        // 中间状态立即返回 Continue：任务保持 RUNNING，由 worker 每 2 秒重新领取轮询，
        // 避免长时间阻塞 worker（选文件/上传/转码可能耗时数分钟）。
        match state {
            SidecarState::Published => {
                finalize_published(ctx, draft_id, asset_id)?;
                Ok(AgentOutcome::Succeeded(json!({
                    "draft_id": draft_id,
                    "asset_id": asset_id,
                    "published": true,
                })))
            }
            SidecarState::Failed(msg) => Err(DomainError::InvalidData(format!(
                "发布 sidecar 失败：{msg}"
            ))),
            SidecarState::BrowserClosed => Ok(AgentOutcome::Cancelled(json!({
                "draft_id": draft_id,
                "asset_id": asset_id,
                "reason": "浏览器标签页已关闭，发布流程中止。",
            }))),
            SidecarState::Cancelled => Ok(AgentOutcome::Cancelled(json!({
                "draft_id": draft_id,
                "asset_id": asset_id,
                "reason": "发布任务已取消。",
            }))),
            SidecarState::WaitingForFile => {
                // 选文件阶段不设人工放行节点：把阶段信息写入任务 payload 供前端展示，
                // 任务保持 RUNNING，由 sidecar 自动检测页面变化并填表。
                TasksRepo::new(ctx.db).update_payload(
                    task.id,
                    json!({
                        "stage": "file",
                        "title": draft.title,
                        "path": asset.file_path,
                    }),
                )?;
                Ok(AgentOutcome::Continue)
            }
            SidecarState::FormFilled => {
                // 幂等：仅 DRAFT -> FILLED。
                let current = DraftsRepo::new(ctx.db).get(draft_id)?.ok_or_else(|| {
                    DomainError::InvalidData(format!("draft {draft_id} not found"))
                })?;
                if current.status == DraftStatus::Draft {
                    DraftsRepo::new(ctx.db).update_status(draft_id, DraftStatus::Filled)?;
                }
                // 用户已点【我已发布】：人工确认发布完成。
                if confirm_path(&ctx.store, draft_id).exists() {
                    finalize_published(ctx, draft_id, asset_id)?;
                    Ok(AgentOutcome::Succeeded(json!({
                        "draft_id": draft_id,
                        "asset_id": asset_id,
                        "published": true,
                        "confirmed_by_human": true,
                    })))
                } else {
                    Ok(waiting_json(
                        "form_ready",
                        draft_id,
                        asset_id,
                        None,
                        "标题/文案/话题已填写完成，标签页保持打开。请核对内容后，由你本人点击页面上的【发布】按钮；发布完成后点击【我已发布】。",
                    ))
                }
            }
            // browser_started / uploading：继续等待下一轮轮询
            SidecarState::BrowserStarted | SidecarState::Uploading | SidecarState::Missing => {
                Ok(AgentOutcome::Continue)
            }
        }
    }
}
