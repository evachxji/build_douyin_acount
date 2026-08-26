use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use serde_json::json;

use crate::agents::{Agent, AgentContext, AgentOutcome};
use crate::db::repos::assets::AssetsRepo;
use crate::db::repos::douyin_posts::DouyinPostsRepo;
use crate::db::repos::drafts::DraftsRepo;
use crate::db::repos::tasks::TasksRepo;
use crate::domain::asset::AssetStatus;
use crate::domain::douyin::NewDouyinPost;
use crate::domain::draft::DraftStatus;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;

/// Publisher Agent：辅助发布到 creator.douyin.com（人机协作）。
///
/// 流程：
/// 1. sidecar 在用户已登录的 Chrome 中新开上传页标签，自动把视频文件注入页面
///    file input（opencli upload，底层 CDP DOM.setFileInputFiles）；注入失败时
///    回退人工选择（waiting_for_file 阶段，前端弹窗展示路径）；
/// 2. 上传/转码完成后 sidecar 自动填写标题/文案/话题/定时/AI 封面并停在
///    【发布】按钮前（form_filled；任务保持 RUNNING，payload stage=form_ready）；
/// 3. 用户在应用侧栏点勾授权（写 confirm 标记），sidecar 据此代点一次发布按钮；
///    用户亦可自行在页面点击发布。「发布已提交」信号（页面跳转/成功提示）命中后
///    本 Agent 标记资产 PUBLISHED 并直接登记发布日历。
/// 4. 多个发布任务可并行：sidecar 会话与 progress/cancel/confirm 标记文件均按
///    draft_id 隔离；定时时间计算把其他活跃任务已选定的时刻计入占位，
///    按三层覆盖策略顺延，不与进行中的发布撞同一时段。
///
/// 【安全红线】发布按钮的点击只能由用户在应用内对单个任务显式点勾触发
/// （sidecar 代点一次，无自动/定时/批量路径）；发布完成状态只由「发布成功」
/// 信号（页面跳转/成功提示，被动检测）产生；识别不到发布按钮元素必须停止
/// 并提示人工，不得盲目操作。
///
/// 【僵尸恢复】sidecar 每 5s 心跳重写进度文件；进程死亡（如随应用重启被杀）则
/// 文件 mtime 冻结。transient 态（browser_started/uploading/waiting_for_file）
/// 超过阈值未更新即重拉 sidecar（同名会话先清理旧标签页），最多 2 次后取消任务。
/// form_filled 同样判定但不重拉（重拉会重新上传填表）：点勾代点依赖 sidecar
/// 存活，冻结直接取消任务并提示用户核实抖音后台状态。
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

/// sidecar 捕获的发布提交信息（页面实际值，用户可能手改过 agent 的预设）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PostCapture {
    /// 发布/草稿接口响应里的真实作品 id；钩子未命中时为空，登记用 local-draft-<id> 占位。
    pub aweme_id: Option<String>,
    pub title: Option<String>,
    /// "YYYY-MM-DD HH:MM"（本地时间）；空 = 立即发布。
    pub schedule_time: Option<String>,
    /// sidecar 落盘的封面文件路径。
    pub cover_path: Option<String>,
}

/// sidecar 进度文件状态。
#[derive(Debug, Clone, PartialEq, Eq)]
enum SidecarState {
    BrowserStarted,
    /// 自动上传失败的兜底态：等待用户手动选择文件，携带 sidecar 提示消息。
    WaitingForFile(String),
    Uploading,
    /// 表单已填完、停在发布前：携带 sidecar 进度消息（透传侧栏行内展示，
    /// 含点勾代点后的结果/失败提醒）。
    FormFilled(String),
    Published(Option<PostCapture>),
    BrowserClosed,
    Cancelled,
    Failed(String),
    Missing,
}

/// 解析 progress.post（sidecar 捕获的发布提交信息）；空字符串字段视为缺失。
fn parse_post_capture(v: &serde_json::Value) -> Option<PostCapture> {
    let p = &v["post"];
    if !p.is_object() {
        return None;
    }
    let s = |k: &str| {
        p[k].as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    Some(PostCapture {
        aweme_id: s("awemeId"),
        title: s("title"),
        schedule_time: s("scheduleTime"),
        cover_path: s("coverPath"),
    })
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
        "form_filled" => SidecarState::FormFilled(
            v["message"].as_str().unwrap_or("").to_string(),
        ),
        "published" => SidecarState::Published(parse_post_capture(&v)),
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

/// 用户在应用侧栏点勾授权发布：写 confirm 标记，sidecar 据此代点一次发布按钮。
pub fn request_confirm(store: &crate::media::MediaStore, draft_id: i64) -> Result<(), DomainError> {
    write_marker(store, "confirm", draft_id)
}

/// 定时发布目标时刻（本地时间），今天起 14 天内按三层覆盖挑选：
/// 1. 第一遍：最近一个「没有公开视频」的日子（发布日历口径，私密不占位），
///    当天 08:00~19:00 随机分钟——与原逻辑一致，先保证每天都有 1 个；
/// 2. 第二遍：14 天都有视频后，最近一个「恰好 1 个视频」的日子，按
///    上午(08:00~12:00)→下午(12:00~19:00)→晚上(19:00~22:00) 顺序取第一个
///    未被占用的时段（已有上午→下午、已有下午→上午的互补由此自然得出）；
/// 3. 第三遍：同理给「恰好 2 个视频」的最近日子补第 3 个。
///
/// 今天（offset=0）还要求起始分钟 ≥ ceil(now+2h)（抖音「2 小时后及 14 天内」），
/// 目标时段今天窗口不足则顺推下一空闲时段（上午/下午不足 → 晚上），三段都不足
/// 则跳过今天。14 天 × 3 全满返回 None，由 sidecar 提示人工选择。
pub fn pick_schedule_time(
    day_minutes: &HashMap<NaiveDate, Vec<i64>>,
    now: NaiveDateTime,
    rand_nanos: u32,
) -> Option<NaiveDateTime> {
    const OPEN_MIN: i64 = 8 * 60; // 08:00
    const NOON_MIN: i64 = 12 * 60; // 12:00
    const CLOSE_MIN: i64 = 19 * 60; // 19:00（原窗口关 = 下午关）
    const NIGHT_CLOSE_MIN: i64 = 22 * 60; // 22:00（晚上关）
    /// 上午 / 下午 / 晚上 时段（含开不含关）。
    const SLOTS: [(i64, i64); 3] = [
        (OPEN_MIN, NOON_MIN),
        (NOON_MIN, CLOSE_MIN),
        (CLOSE_MIN, NIGHT_CLOSE_MIN),
    ];
    /// 时刻归段：段外时刻归最近段（如 07:30→上午、23:00→晚上）。
    fn slot_of(minute: i64) -> usize {
        if minute < NOON_MIN {
            0
        } else if minute < CLOSE_MIN {
            1
        } else {
            2
        }
    }

    let today = now.date();
    // 今天可选的最早分钟（抖音限制不早于 2 小时后，向上取整）；
    // now+2h 跨天则今天任何时刻都违反限制，置 MAX 使今天全部时段不可选。
    let earliest = now + chrono::Duration::hours(2);
    let earliest_today = if earliest.date() == today {
        i64::from(earliest.num_seconds_from_midnight().div_ceil(60))
    } else {
        i64::MAX
    };
    let pick_in = |day: NaiveDate, start_min: i64, end_min: i64| {
        let span = (end_min - start_min) as u32;
        let minute = start_min + i64::from(rand_nanos % span);
        day.and_hms_opt(0, 0, 0)
            .map(|base| base + chrono::Duration::minutes(minute))
    };

    // 第一遍：空缺日优先（原逻辑）。
    for offset in 0..14 {
        let day = today + chrono::Duration::days(offset);
        if day_minutes.get(&day).is_some_and(|v| !v.is_empty()) {
            continue;
        }
        let start_min = if offset == 0 {
            OPEN_MIN.max(earliest_today)
        } else {
            OPEN_MIN
        };
        if start_min < CLOSE_MIN {
            return pick_in(day, start_min, CLOSE_MIN);
        }
    }

    // 第二、三遍：恰好 want 个视频的最近日子，补顺序第一个未占用且可选择的时段。
    for want in 1..=2usize {
        for offset in 0..14 {
            let day = today + chrono::Duration::days(offset);
            let Some(times) = day_minutes.get(&day) else {
                continue;
            };
            if times.len() != want {
                continue;
            }
            let mut taken = [false; 3];
            for &m in times {
                taken[slot_of(m)] = true;
            }
            for (idx, &(slot_open, slot_close)) in SLOTS.iter().enumerate() {
                if taken[idx] {
                    continue;
                }
                let start_min = if offset == 0 {
                    slot_open.max(earliest_today)
                } else {
                    slot_open
                };
                if start_min < slot_close {
                    return pick_in(day, start_min, slot_close);
                }
            }
        }
    }
    None
}

fn spawn_sidecar(
    ctx: &AgentContext,
    task_id: i64,
    draft_id: i64,
    asset: &crate::domain::asset::VideoAsset,
    draft: &crate::domain::draft::PublishDraft,
    self_agent: &PublisherAgent,
) -> Result<(), DomainError> {
    let base = publish_dir(&ctx.store);
    let jobs = base.join("jobs");
    std::fs::create_dir_all(&jobs)?;
    let job_path = jobs.join(format!("{draft_id}.json"));
    // 定时发布：按日历 14 天三层覆盖（1→2→3 个/天）计算目标时刻（None = 全满，
    // sidecar 会提示人工选择）。
    let mut day_minutes: HashMap<NaiveDate, Vec<i64>> = HashMap::new();
    for (d, m) in DouyinPostsRepo::new(ctx.db).list_day_video_minutes()? {
        day_minutes.entry(d).or_default().push(m);
    }
    // 并发适配：其他活跃发布任务已选定的定时时刻计入占位（排除本任务——僵尸
    // 重拉时自己的 payload 已有旧值），后续任务按三层覆盖策略顺延，不与进行中的
    // 发布撞同一时段。任务 finalize 前其定时不在 douyin_posts 日历里，必须在此补位。
    for s in TasksRepo::new(ctx.db).list_active_schedule_times(task_id)? {
        if let Some(ts) = parse_local_ts(&s) {
            if let Some(ndt) = Local.timestamp_opt(ts, 0).single().map(|d| d.naive_local()) {
                day_minutes
                    .entry(ndt.date())
                    .or_default()
                    .push(i64::from(ndt.num_seconds_from_midnight() / 60));
            }
        }
    }
    let now = Local::now();
    let schedule_time = pick_schedule_time(&day_minutes, now.naive_local(), now.timestamp_subsec_nanos());
    // 名称与定时时刻写入任务 payload，侧栏发布列表据此展示（首轮 spawn 后即可见）。
    TasksRepo::new(ctx.db).update_payload(
        task_id,
        json!({
            "title": draft.title,
            "schedule_time": schedule_time.map(|t| t.format("%Y-%m-%d %H:%M").to_string()),
        }),
    )?;
    // 自主声明开关（每次 spawn 现读，设置改动即时生效）；读取失败回退开启（历史行为）。
    let declaration = crate::settings::SettingsRepo::new(ctx.db)
        .load()
        .map(|s| s.publish_declaration)
        .unwrap_or(true);
    let job = json!({
        "draftId": draft_id,
        "videoPath": asset.file_path,
        "title": draft.title,
        "body": draft.body,
        "hashtags": draft.hashtags,
        "declaration": declaration,
        "scheduleTime": schedule_time.map(|t| t.format("%Y-%m-%d %H:%M").to_string()),
        "progressFile": progress_path(&ctx.store, draft_id),
        "cancelFile": cancel_path(&ctx.store, draft_id),
        "confirmFile": confirm_path(&ctx.store, draft_id),
        "coverDir": ctx.store.covers_dir(),
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

fn finalize_published(
    ctx: &AgentContext,
    draft_id: i64,
    asset_id: i64,
    capture: Option<PostCapture>,
) -> Result<(), DomainError> {
    DraftsRepo::new(ctx.db).update_status(draft_id, DraftStatus::Published)?;
    AssetsRepo::new(ctx.db).update_status(asset_id, AssetStatus::Published)?;
    register_local_post(ctx, draft_id, capture)
}

/// 发布完成后直接把作品登记进 douyin_posts，发布日历即时更新（不再跑整轮 douyin_sync）。
/// 字段优先取 sidecar 捕获的页面实际值（用户可能手改过标题/定时），缺省回退 draft/job.json；
/// 作品 id 缺失时用 local-draft-<draft_id> 占位，下次整轮 sync 拉到真实行后清理
/// （create_time 精确相等，或同日+占位标题前缀——见 delete_local_placeholders）。
fn register_local_post(
    ctx: &AgentContext,
    draft_id: i64,
    capture: Option<PostCapture>,
) -> Result<(), DomainError> {
    let draft = DraftsRepo::new(ctx.db)
        .get(draft_id)?
        .ok_or_else(|| DomainError::InvalidData(format!("draft {draft_id} not found")))?;
    let cap = capture.unwrap_or_default();
    let schedule_str = cap
        .schedule_time
        .or_else(|| job_schedule_time(&ctx.store, draft_id));
    let schedule_ts = schedule_str.as_deref().and_then(parse_local_ts);
    let now = Local::now().timestamp();
    let (status, create_time, public_time) = match schedule_ts {
        Some(ts) if ts > now => ("scheduled", ts, 0),
        // 定时时间已过（提交后停留太久才判定）按已发布记。
        Some(ts) => ("published", ts, ts),
        None => ("published", now, now),
    };
    let aweme_id = cap
        .aweme_id
        .unwrap_or_else(|| format!("local-draft-{draft_id}"));
    let repo = DouyinPostsRepo::new(ctx.db);
    // 真实行若已被整轮 sync 抢先拉取（带统计数据），不覆盖。
    if !aweme_id.starts_with("local-") && repo.get_by_aweme(&aweme_id)?.is_some() {
        return Ok(());
    }
    repo.upsert(&NewDouyinPost {
        aweme_id,
        title: cap.title.or(draft.title),
        cover_path: cap.cover_path,
        cover_url: None,
        status: status.to_string(),
        play_count: 0,
        digg_count: 0,
        comment_count: 0,
        share_count: 0,
        completion_rate: None,
        bounce_rate_2s: None,
        violation_status: None,
        violation_text: None,
        violation_brief: None,
        violation_url: None,
        create_time,
        public_time,
    })?;
    Ok(())
}

/// 读取 spawn 时写入 job.json 的定时时间（sidecar 捕获缺失时的兜底）。
fn job_schedule_time(store: &crate::media::MediaStore, draft_id: i64) -> Option<String> {
    let path = publish_dir(store).join("jobs").join(format!("{draft_id}.json"));
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v["scheduleTime"].as_str().map(String::from)
}

/// "YYYY-MM-DD HH:MM"（本地时间）→ epoch 秒。
fn parse_local_ts(s: &str) -> Option<i64> {
    let ndt = NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M").ok()?;
    Local.from_local_datetime(&ndt).single().map(|dt| dt.timestamp())
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
            spawn_sidecar(ctx, task.id, draft_id, &asset, &draft, self)?;
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
            spawn_sidecar(ctx, task.id, draft_id, &asset, &draft, self)?;
            return Ok(AgentOutcome::Continue);
        }

        // form_filled 心跳判定：点勾代点依赖 sidecar 存活，冻结视为进程已死。
        // 不重拉（重拉会重新上传填表、且用户可能已在页面手动发布）——直接取消
        // 并提示核实抖音后台状态，素材保持 SELECTED 可重新发起。
        if matches!(state, SidecarState::FormFilled(_))
            && progress_stale(&progress_file, self.stale_after)
        {
            return Ok(AgentOutcome::Cancelled(json!({
                "draft_id": draft_id,
                "asset_id": asset_id,
                "reason": "发布 sidecar 已失联（可能随应用重启终止）。如刚已确认发布，请先到抖音后台核实作品状态；未发布请重新点击【准备发布】。",
            })));
        }

        // 中间状态立即返回 Continue：任务保持 RUNNING，由 worker 每 2 秒重新领取轮询，
        // 避免长时间阻塞 worker（选文件/上传/转码可能耗时数分钟）。
        match state {
            SidecarState::Published(post) => {
                finalize_published(ctx, draft_id, asset_id, post)?;
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
            SidecarState::FormFilled(message) => {
                // 幂等：仅 DRAFT -> FILLED。
                let current = DraftsRepo::new(ctx.db).get(draft_id)?.ok_or_else(|| {
                    DomainError::InvalidData(format!("draft {draft_id} not found"))
                })?;
                if current.status == DraftStatus::Draft {
                    DraftsRepo::new(ctx.db).update_status(draft_id, DraftStatus::Filled)?;
                }
                // 任务保持 RUNNING 等待用户在侧栏点勾（confirm 标记由 sidecar 响应，
                // 代点一次发布按钮）或自行在页面发布；sidecar 进度消息透传到行内展示
                // （含「已按你的确认点击发布」/点击失败提醒）。
                TasksRepo::new(ctx.db).update_payload(
                    task.id,
                    json!({ "stage": "form_ready", "message": message }),
                )?;
                Ok(AgentOutcome::Continue)
            }
            // browser_started / uploading：继续等待下一轮轮询
            SidecarState::BrowserStarted | SidecarState::Uploading | SidecarState::Missing => {
                Ok(AgentOutcome::Continue)
            }
        }
    }
}
