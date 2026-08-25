use serde_json::json;
use tauri::State;

use crate::db::repos::analyses::AnalysesRepo;
use crate::db::repos::assets::AssetsRepo;
use crate::db::repos::drafts::DraftsRepo;
use crate::db::repos::tasks::TasksRepo;
use crate::domain::asset::AssetStatus;
use crate::domain::draft::NewDraft;
use crate::domain::error::{ApiError, DomainError};
use crate::state::AppState;

/// 抖音标题上限（字符）。
const TITLE_MAX_CHARS: usize = 30;

/// 「准备发布」：为 SELECTED 素材生成发布草稿（标题/文案/话题取自 AI 分析），
/// 并入队 publisher 任务（sidecar 自动填表，最终发布由用户本人点击）。
#[tauri::command]
pub fn prepare_publish(
    state: State<AppState>,
    asset_id: i64,
) -> Result<serde_json::Value, ApiError> {
    let db = &state.db;
    let assets = AssetsRepo::new(db);
    let asset = assets
        .get(asset_id)?
        .ok_or(DomainError::AssetNotFound(asset_id))?;
    if asset.status != AssetStatus::Selected {
        return Err(DomainError::InvalidData(
            "只有「待发布」(SELECTED) 的素材可以准备发布，请先加入待发布。".to_string(),
        )
        .into());
    }

    // 优先使用已生成的风格化文案（标题/正文/话题完整）。
    if let Some(copy) = crate::db::repos::content::ContentRepo::new(db).get_by_asset(asset_id)? {
        let drafts = DraftsRepo::new(db);
        let draft_id = drafts.create(NewDraft {
            asset_id,
            style_id: copy.style_id,
            title: Some(copy.title.clone()),
            body: Some(copy.body),
            hashtags: copy.hashtags,
        })?;
        let task_id = TasksRepo::new(db).enqueue(
            "publisher",
            "prepare_publish",
            json!({ "draft_id": draft_id, "asset_id": asset_id, "title": copy.title }),
            true,
        )?;
        let draft = drafts
            .get(draft_id)?
            .ok_or(DomainError::InvalidData("draft disappeared".to_string()))?;
        return Ok(json!({ "draft": draft, "task_id": task_id }));
    }

    let analysis = AnalysesRepo::new(db).get_by_asset(asset_id)?;
    let caption = analysis
        .as_ref()
        .and_then(|a| a.recommended_caption.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let summary = analysis
        .as_ref()
        .and_then(|a| a.summary.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let tags: Vec<String> = analysis
        .as_ref()
        .map(|a| a.tags.clone())
        .unwrap_or_default();

    // 标题：优先推荐文案第一行截断到 30 字，其次素材标题。
    let title_hint = caption
        .or(asset.title.as_deref())
        .unwrap_or("未命名视频");
    let first_line = title_hint.split('\n').next().unwrap_or(title_hint).trim();
    let title: String = first_line.chars().take(TITLE_MAX_CHARS).collect();
    // 正文：完整推荐文案，其次 AI 总结。
    let body = caption
        .map(str::to_string)
        .or_else(|| summary.map(str::to_string))
        .unwrap_or_else(|| title.clone());

    let drafts = DraftsRepo::new(db);
    let draft_id = drafts.create(NewDraft {
        asset_id,
        style_id: None,
        title: Some(title.clone()),
        body: Some(body),
        hashtags: tags,
    })?;
    let task_id = TasksRepo::new(db).enqueue(
        "publisher",
        "prepare_publish",
        json!({ "draft_id": draft_id, "asset_id": asset_id, "title": title }),
        true,
    )?;
    let draft = drafts
        .get(draft_id)?
        .ok_or(DomainError::InvalidData("draft disappeared".to_string()))?;
    Ok(json!({ "draft": draft, "task_id": task_id }))
}

/// 发布草稿列表（按创建时间倒序）。
#[tauri::command]
pub fn list_drafts(state: State<AppState>) -> Result<Vec<crate::domain::draft::PublishDraft>, ApiError> {
    DraftsRepo::new(&state.db).list().map_err(ApiError::from)
}

/// 用户在侧栏点勾授权发布：写入 confirm 标记，sidecar 轮询到后据此在页面
/// 代点一次发布按钮（任务全程保持 RUNNING，无需放行）。
#[tauri::command]
pub fn publish_confirmed(state: State<AppState>, draft_id: i64) -> Result<(), ApiError> {
    crate::agents::publisher::request_confirm(&state.store, draft_id).map_err(ApiError::from)
}

/// 把文本写入系统剪贴板（文件选择阶段自动复制视频路径，用户直接粘贴）。
#[tauri::command]
pub fn copy_to_clipboard(text: String) -> Result<(), ApiError> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| {
        ApiError::from(DomainError::InvalidData(format!("clipboard error: {e}")))
    })?;
    clipboard.set_text(text).map_err(|e| {
        ApiError::from(DomainError::InvalidData(format!("clipboard error: {e}")))
    })
}
