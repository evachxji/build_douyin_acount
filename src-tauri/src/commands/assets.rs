use std::path::PathBuf;

use tauri::State;

use crate::db::repos::analyses::AnalysesRepo;
use crate::db::repos::assets::{AssetFilter, AssetsRepo};
use crate::domain::analysis::AnalysisResult;
use crate::domain::asset::{AssetStatus, VideoAsset};
use crate::domain::error::{ApiError, DomainError};
use crate::media::VideoManager;
use crate::state::AppState;

/// 资产列表筛选参数（前端透传，缺省即不过滤）。
#[derive(Debug, serde::Deserialize)]
pub struct AssetListFilterArg {
    pub category: Option<String>,
    pub statuses: Option<Vec<String>>,
    pub platform: Option<String>,
    pub search: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[tauri::command]
pub fn list_assets(
    state: State<AppState>,
    filter: AssetListFilterArg,
) -> Result<Vec<VideoAsset>, ApiError> {
    let statuses = filter
        .statuses
        .unwrap_or_default()
        .iter()
        .map(|s| s.parse::<AssetStatus>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::from)?;
    let filter = AssetFilter {
        statuses,
        platform: filter.platform,
        category: filter.category,
        search: filter.search,
        limit: filter.limit.unwrap_or(300),
        offset: filter.offset.unwrap_or(0),
    };
    AssetsRepo::new(&state.db).list(filter).map_err(ApiError::from)
}

#[tauri::command]
pub fn get_asset(state: State<AppState>, id: i64) -> Result<Option<VideoAsset>, ApiError> {
    VideoManager::new(&state.db, state.store.clone())
        .get_asset(id)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn get_analysis(
    state: State<AppState>,
    asset_id: i64,
) -> Result<Option<AnalysisResult>, ApiError> {
    AnalysesRepo::new(&state.db)
        .get_by_asset(asset_id)
        .map_err(ApiError::from)
}

#[tauri::command]
pub fn delete_asset(state: State<AppState>, id: i64) -> Result<(), ApiError> {
    VideoManager::new(&state.db, state.store.clone())
        .delete_asset(id)
        .map_err(ApiError::from)
}

/// 状态流转（合法性由状态机校验）；手工标记淘汰时记录「手工淘汰」，离开淘汰态时清除原因。
/// 淘汰时级联取消该资产排队中/进行中的分析任务（无需再为其浪费分析资源）。
#[tauri::command]
pub fn update_asset_status(
    state: State<AppState>,
    id: i64,
    to: String,
) -> Result<(), ApiError> {
    let status: AssetStatus = to.parse().map_err(ApiError::from)?;
    VideoManager::new(&state.db, state.store.clone()).update_status(id, status)?;
    let reason = if status == AssetStatus::Eliminated {
        Some("手工淘汰")
    } else {
        None
    };
    AssetsRepo::new(&state.db).set_eliminate_reason(id, reason)?;
    if status == AssetStatus::Eliminated {
        crate::db::repos::tasks::TasksRepo::new(&state.db).cancel_active_analysis_for_asset(id)?;
    }
    Ok(())
}

/// 在系统文件管理器中定位并选中视频文件（Windows 资源管理器 /select）。
#[tauri::command]
pub fn reveal_in_folder(path: String) -> Result<(), ApiError> {
    reveal_in_folder_impl(&path).map_err(ApiError::from)
}

fn reveal_in_folder_impl(path: &str) -> Result<(), DomainError> {
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err(DomainError::InvalidData(format!(
            "file not found: {}",
            path.display()
        )));
    }
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn()?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg("-R").arg(&path).spawn()?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path.as_path()))
        .spawn()?;
    Ok(())
}

#[derive(Debug, serde::Serialize)]
pub struct CategoryCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

/// 左侧分类树 + 顶栏需要的一次性总览数据。
#[derive(Debug, serde::Serialize)]
pub struct LibraryOverview {
    pub total: i64,
    pub categories: Vec<CategoryCount>,
    pub statuses: Vec<StatusCount>,
}

#[tauri::command]
pub fn library_overview(state: State<AppState>) -> Result<LibraryOverview, ApiError> {
    let repo = AssetsRepo::new(&state.db);
    let categories = repo
        .category_counts()
        .map_err(ApiError::from)?
        .into_iter()
        .map(|(name, count)| CategoryCount { name, count })
        .collect::<Vec<_>>();
    let statuses = repo
        .status_counts()
        .map_err(ApiError::from)?
        .into_iter()
        .map(|(status, count)| StatusCount { status, count })
        .collect::<Vec<_>>();
    let total = statuses.iter().map(|s| s.count).sum();
    Ok(LibraryOverview {
        total,
        categories,
        statuses,
    })
}
