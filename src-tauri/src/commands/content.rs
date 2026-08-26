use serde_json::json;
use tauri::State;

use crate::db::repos::analyses::AnalysesRepo;
use crate::db::repos::assets::AssetsRepo;
use crate::db::repos::content::ContentRepo;
use crate::db::repos::tasks::TasksRepo;
use crate::domain::asset::AssetStatus;
use crate::domain::error::{ApiError, DomainError};
use crate::state::AppState;

/// 每个分类最多推荐条数。
const TOP_PER_CATEGORY: usize = 3;

/// 为单个素材入队文案生成任务（按当前设置的账号风格）。
#[tauri::command]
pub fn generate_copy(state: State<AppState>, asset_id: i64) -> Result<i64, ApiError> {
    let style = state
        .settings
        .read()
        .expect("settings lock poisoned")
        .account_style
        .clone();
    let task_id = TasksRepo::new(&state.db).enqueue(
        "content",
        "generate_copy",
        json!({ "asset_id": asset_id, "style": style }),
        false,
    )?;
    Ok(task_id)
}

/// 读取素材已生成的风格化文案。
#[tauri::command]
pub fn get_copy(
    state: State<AppState>,
    asset_id: i64,
) -> Result<Option<crate::domain::content::ContentCopy>, ApiError> {
    ContentRepo::new(&state.db)
        .get_by_asset(asset_id)
        .map_err(ApiError::from)
}

/// 详情页双击编辑文案：更新已生成文案的标题/正文（话题不变）。
/// 返回更新后的完整文案；素材尚无文案时报错（前端只在有文案时才开放编辑）。
#[tauri::command]
pub fn update_copy(
    state: State<AppState>,
    asset_id: i64,
    title: String,
    body: String,
) -> Result<crate::domain::content::ContentCopy, ApiError> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(DomainError::InvalidData("标题不能为空".to_string()).into());
    }
    ContentRepo::new(&state.db)
        .update_text(asset_id, &title, body.trim())?
        .ok_or_else(|| DomainError::InvalidData("该素材尚未生成文案".to_string()).into())
}

/// 推荐打分核心：质量分加权 + 点赞对数归一 + 作者多样性；写入 recommendations 表，
/// 并为入选素材自动入队文案生成。抽出为纯函数便于测试。
pub fn recommend(db: &crate::db::Database, style: &str) -> Result<serde_json::Value, DomainError> {
    let assets = AssetsRepo::new(db);
    // READY 且已分析的素材
    let rows = assets.list(crate::db::repos::assets::AssetFilter {
        statuses: vec![AssetStatus::Candidate],
        ..Default::default()
    })?;
    let analyses = AnalysesRepo::new(db);
    let mut groups: std::collections::BTreeMap<String, Vec<serde_json::Value>> =
        std::collections::BTreeMap::new();

    for asset in rows {
        let analysis = analyses.get_by_asset(asset.id)?;
        let category = analysis
            .as_ref()
            .and_then(|a| a.category.clone())
            .unwrap_or_else(|| "未分类".to_string());
        let quality = analysis
            .as_ref()
            .and_then(|a| a.quality_score)
            .unwrap_or(0)
            .clamp(0, 100) as f64;
        let likes = asset.likes.max(0) as f64;
        let like_norm = ((likes + 1.0).log10() / 7.0).min(1.0);
        let score = 0.6 * (quality / 100.0) + 0.3 * like_norm + 0.1;
        let reason = format!(
            "质量{} · 点赞{}",
            analysis.as_ref().and_then(|a| a.quality_score).unwrap_or(0),
            format_count(asset.likes)
        );
        groups.entry(category.clone()).or_default().push(json!({
            "asset_id": asset.id,
            "title": asset.title,
            "author": asset.author,
            "likes": asset.likes,
            "quality_score": analysis.as_ref().and_then(|a| a.quality_score),
            "score": (score * 100.0).round() / 100.0,
            "category": category,
            "reason": reason,
        }));
    }

    // 组内排序（分数降序），同一作者最多入选 1 条（多样性），取每类 TOP_N。
    let batch_id = uuid::Uuid::new_v4().to_string();
    let mut output_groups = Vec::new();
    let tasks = TasksRepo::new(db);
    for (category, mut items) in groups {
        items.sort_by(|a, b| {
            b["score"]
                .as_f64()
                .unwrap_or(0.0)
                .partial_cmp(&a["score"].as_f64().unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut seen_authors = std::collections::HashSet::new();
        let mut picked = Vec::new();
        for item in &items {
            if picked.len() >= TOP_PER_CATEGORY {
                break;
            }
            let author = item["author"].as_str().unwrap_or("");
            if !author.is_empty() && !seen_authors.insert(author.to_string()) {
                continue;
            }
            picked.push(item.clone());
        }
        // 分类过小时允许重复作者补足
        if picked.len() < TOP_PER_CATEGORY {
            for item in &items {
                if picked.len() >= TOP_PER_CATEGORY {
                    break;
                }
                if !picked.contains(item) {
                    picked.push(item.clone());
                }
            }
        }
        for (rank, item) in picked.iter().enumerate() {
            let asset_id = item["asset_id"].as_i64().unwrap_or(0);
            let reason = format!(
                "质量{} · 点赞{}",
                item["quality_score"].as_i64().unwrap_or(0),
                format_count(item["likes"].as_i64().unwrap_or(0))
            );
            db.conn()
                .execute(
                "INSERT INTO recommendations (batch_id, asset_id, category, rank, reason, score)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                rusqlite::params![
                    batch_id,
                    asset_id,
                    category,
                    rank as i64 + 1,
                    reason,
                    item["score"].as_f64().unwrap_or(0.0)
                ],
                )
                .map_err(DomainError::from)?;
            // 自动入队文案生成
            tasks.enqueue(
                "content",
                "generate_copy",
                json!({ "asset_id": asset_id, "style": style }),
                false,
            )?;
        }
        output_groups.push(json!({
            "category": category,
            "items": picked,
        }));
    }
    Ok(json!({ "batch_id": batch_id, "groups": output_groups }))
}

/// 推荐打分：质量分加权 + 点赞对数归一 + 作者多样性；写入 recommendations 表，
/// 并为入选素材自动入队文案生成。
#[tauri::command]
pub fn get_recommendations(state: State<AppState>) -> Result<serde_json::Value, ApiError> {
    let style = state
        .settings
        .read()
        .expect("settings lock poisoned")
        .account_style
        .clone();
    recommend(&state.db, &style).map_err(ApiError::from)
}

fn format_count(n: i64) -> String {
    if n >= 10_000 {
        format!("{:.1}万", n as f64 / 10_000.0)
    } else {
        n.to_string()
    }
}
