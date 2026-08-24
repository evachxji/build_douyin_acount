use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use chrono::{Local, NaiveDate, NaiveDateTime, Timelike};
use serde_json::json;

use crate::agents::{Agent, AgentContext, AgentOutcome};
use crate::db::repos::assets::AssetsRepo;
use crate::db::repos::douyin_posts::DouyinPostsRepo;
use crate::db::repos::drafts::DraftsRepo;
use crate::db::repos::tasks::TasksRepo;
use crate::domain::asset::AssetStatus;
use crate::domain::draft::DraftStatus;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;

/// Publisher Agent：辅助发布到 creator.douyin.com（人机协作）。
///
/// 流程：
/// 1. sidecar 在用户已登录的 Chrome 中新开上传页标签，自动把视频文件注入页面
///    file input（opencli upload，底层 CDP DOM.setFileInputFiles）；注入失败时
///    回退人工选择（waiting_for_file 阶段，前端弹窗展示路径）；
/// 2. 上传/转码完成后 sidecar 自动填写标题/文案/话题并停在【发布】按钮前
///    （WAITING_FOR_HUMAN）；
/// 3. 用户核对后在浏览器中本人点击发布，再点应用内【我已发布】（写入 confirm 标记），
///    本 Agent 据此标记资产 PUBLISHED。
///
/// 【安全红线】本 Agent 与 sidecar 永不点击最终发布按钮；发布完成状态只由
/// 显式「发布成功」提示或用户本人确认产生。
///
/// 【僵尸恢复】sidecar 每 5s 心跳重写进度文件；进程死亡（如随应用重启被杀）则
/// 文件 mtime 冻结。transient 态（browser_started/uploading/waiting_for_file）
/// 超过阈值未更新即重拉 sidecar（同名会话先清理旧标签页），最多 2 次后取消任务。
/// form_filled 不做此判定：那是人工等待态，确认/取消走标记文件，不依赖 sidecar 存活。
pub struct PublisherAgent {
    node: String,
    script: PathBuf,
    /// 进度文件心跳超时阈值（sidecar 心跳 5s，容忍多次丢失）。
    stale_after: Duration,
}

impl PublisherAgent {
    pub fn new(node: impl Into<String>, script: PathBuf) -> Self {
        Self {
            node: node.into(),
            script,
            stale_after: Duration::from_secs(45),
        }
    }

    /// 测试注入：阈值归零可立即触发僵尸判定路径。
    pub fn with_stale_after(mut self, d: Duration) -> Self {
        self.stale_after = d;
        self
    }
}

/// sidecar 进度文件状态。
#[derive(Debug, Clone, PartialEq, Eq)]
enum SidecarState {
    BrowserStarted,
    /// 自动上传失败的兜底态：等待用户手动选择文件，携带 sidecar 提示消息。
    WaitingForFile(String),
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
        "waiting_for_file" => SidecarState::WaitingForFile(
            v["message"].as_str().unwrap_or("").to_string(),
        ),
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

/// 进度文件是否超过阈值未更新（mtime 冻结 = sidecar 进程已死）；文件不存在不算冻结。
fn progress_stale(path: &Path, threshold: Duration) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().unwrap_or_default() > threshold)
        .unwrap_or(false)
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

/// 定时发布目标时刻（本地时间）：今天起 14 天内、最近一个「没有公开视频」的日子
/// （发布日历口径，私密作品不占位），时刻取当天 08:00~19:00 间随机分钟，且满足抖音
/// 「2 小时后及 14 天内」限制——今天窗口（now+2h 向上取整 ~ 19:00）不足则跳过今天。
/// 14 天均无空档时返回 None，由 sidecar 提示人工选择。
pub fn pick_schedule_time(
    occupied: &HashSet<NaiveDate>,
    now: NaiveDateTime,
    rand_nanos: u32,
) -> Option<NaiveDateTime> {
    const OPEN_MIN: i64 = 8 * 60; // 08:00
    const CLOSE_MIN: i64 = 19 * 60; // 19:00（不含）
    let today = now.date();
    for offset in 0..14 {
        let day = today + chrono::Duration::days(offset);
        if occupied.contains(&day) {
            continue;
        }
        let mut start_min = OPEN_MIN;
        if offset == 0 {
            let earliest = now + chrono::Duration::hours(2);
            let ceil_min = earliest.num_seconds_from_midnight().div_ceil(60);
            start_min = start_min.max(i64::from(ceil_min));
        }
        if start_min >= CLOSE_MIN {
            continue;
        }
        let span = (CLOSE_MIN - start_min) as u32;
        let minute = start_min + i64::from(rand_nanos % span);
        return day
            .and_hms_opt(0, 0, 0)
            .map(|base| base + chrono::Duration::minutes(minute));
    }
    None
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
    // 定时发布：按日历空档日 + 08:00~19:00 随机分钟计算目标时刻（None = 14 天无空档，
    // sidecar 会提示人工选择）。
    let occupied = DouyinPostsRepo::new(ctx.db).list_occupied_days()?;
    let now = Local::now();
    let schedule_time = pick_schedule_time(&occupied, now.naive_local(), now.timestamp_subsec_nanos());
    let job = json!({
        "draftId": draft_id,
        "videoPath": asset.file_path,
        "title": draft.title,
        "body": draft.body,
        "hashtags": draft.hashtags,
        "scheduleTime": schedule_time.map(|t| t.format("%Y-%m-%d %H:%M").to_string()),
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

        // 僵尸恢复：transient 态进度文件超过阈值未更新（sidecar 已死，如应用重启时
        // 被杀）则重拉 sidecar——同名会话的 closeSession 会先清理旧标签页，不会开重复页。
        // 最多重拉 2 次，仍无心跳则取消任务（素材保持 SELECTED，可重新点击【准备发布】）。
        // form_filled 与终态不在此列（确认/取消走标记文件，不依赖 sidecar 存活）；
        // waiting_for_file 虽是人工等待，但 sidecar 必须活着监听页面变化，故纳入判定。
        if matches!(
            state,
            SidecarState::BrowserStarted | SidecarState::Uploading | SidecarState::WaitingForFile(_)
        ) && progress_stale(&progress_file, self.stale_after)
        {
            let respawns = task.payload["respawns"].as_i64().unwrap_or(0);
            if respawns >= 2 {
                return Ok(AgentOutcome::Cancelled(json!({
                    "draft_id": draft_id,
                    "asset_id": asset_id,
                    "reason": "发布 sidecar 无响应（可能已随应用重启终止），请重新点击【准备发布】。",
                })));
            }
            TasksRepo::new(ctx.db)
                .update_payload(task.id, json!({ "respawns": respawns + 1 }))?;
            spawn_sidecar(ctx, draft_id, &asset, &draft, self)?;
            return Ok(AgentOutcome::Continue);
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
            SidecarState::WaitingForFile(message) => {
                // 选文件阶段不设人工放行节点：把阶段信息写入任务 payload 供前端展示，
                // 任务保持 RUNNING，由 sidecar 自动检测页面变化并填表。
                TasksRepo::new(ctx.db).update_payload(
                    task.id,
                    json!({
                        "stage": "file",
                        "title": draft.title,
                        "path": asset.file_path,
                        "message": message,
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
