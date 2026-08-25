use serde_json::json;
use tauri::State;

use crate::db::repos::douyin_posts::DouyinPostsRepo;
use crate::db::repos::tasks::TasksRepo;
use crate::domain::douyin::DouyinPost;
use crate::domain::error::{ApiError, DomainError};
use crate::settings::SettingsRepo;
use crate::state::AppState;

/// 发起一轮抖音作品数据同步（创作者中心作品列表 → douyin_posts 表）。
/// 防并发：已有同步任务在进行/排队时拒绝重复发起。
#[tauri::command]
pub fn start_douyin_sync(state: State<AppState>) -> Result<i64, ApiError> {
    let repo = TasksRepo::new(&state.db);
    if repo.has_active("douyin_sync")? {
        return Err(DomainError::InvalidData(
            "已有同步任务在进行或排队中，请先等待完成或取消。".to_string(),
        )
        .into());
    }
    repo.enqueue("douyin_sync", "sync_works", json!({}), false)
        .map_err(ApiError::from)
}

/// 自动巡检开关：开启后每 30 分钟自动同步一轮，发现违规作品自动处理。
#[tauri::command]
pub fn get_auto_sync_moderate(state: State<AppState>) -> Result<bool, ApiError> {
    SettingsRepo::new(&state.db)
        .get_auto_sync_moderate()
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn set_auto_sync_moderate(state: State<AppState>, enabled: bool) -> Result<(), ApiError> {
    SettingsRepo::new(&state.db)
        .set_auto_sync_moderate(enabled)
        .map_err(ApiError::from)
}

/// 发布日历数据：已同步的抖音作品快照（按发布时间倒序）。
#[tauri::command]
pub fn list_douyin_posts(state: State<AppState>) -> Result<Vec<DouyinPost>, ApiError> {
    DouyinPostsRepo::new(&state.db).list(500).map_err(ApiError::from)
}

/// 处理单条违规作品：设为仅自己可见；若今天发布且有定时作品，最晚一条定时改为立即发布。
#[tauri::command]
pub fn moderate_douyin_violation(state: State<AppState>, aweme_id: String) -> Result<i64, ApiError> {
    let repo = DouyinPostsRepo::new(&state.db);
    let post = repo
        .get_by_aweme(&aweme_id)?
        .ok_or_else(|| DomainError::InvalidData(format!("作品不存在: {aweme_id}")))?;
    if post.violation_text.is_none() {
        return Err(DomainError::InvalidData("该作品当前没有违规标记".to_string()).into());
    }
    if post.status == "private" {
        return Err(DomainError::InvalidData("该作品已是私密状态，无需处理".to_string()).into());
    }
    let tasks = TasksRepo::new(&state.db);
    if tasks.has_active("douyin_moderate")? {
        return Err(DomainError::InvalidData(
            "已有违规处理任务在进行或排队中，请先等待完成或取消。".to_string(),
        )
        .into());
    }
    tasks
        .enqueue(
            "douyin_moderate",
            "moderate_violation",
            json!({"mode": "single", "aweme_id": aweme_id}),
            false,
        )
        .map_err(ApiError::from)
}

/// 处理全部违规作品：逐条设为仅自己可见；今天发布的各触发一条最晚定时作品立即发布。
#[tauri::command]
pub fn moderate_all_violations(state: State<AppState>) -> Result<i64, ApiError> {
    let repo = DouyinPostsRepo::new(&state.db);
    let n = repo.list_violating()?.len();
    if n == 0 {
        return Err(DomainError::InvalidData("当前没有违规作品需要处理".to_string()).into());
    }
    let tasks = TasksRepo::new(&state.db);
    if tasks.has_active("douyin_moderate")? {
        return Err(DomainError::InvalidData(
            "已有违规处理任务在进行或排队中，请先等待完成或取消。".to_string(),
        )
        .into());
    }
    tasks
        .enqueue(
            "douyin_moderate",
            "moderate_violation",
            json!({"mode": "all"}),
            false,
        )
        .map_err(ApiError::from)
}

/// 在系统默认浏览器打开 URL（作品详情页/定时编辑页）。
#[tauri::command]
pub fn open_url(url: String) -> Result<(), ApiError> {
    if !url.starts_with("https://creator.douyin.com/") {
        return Err(DomainError::InvalidData(format!("仅允许打开创作者中心链接: {url}")).into());
    }
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&url)
        .spawn()
        .map_err(DomainError::from)?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(DomainError::from)?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    std::process::Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .map_err(DomainError::from)?;
    Ok(())
}
