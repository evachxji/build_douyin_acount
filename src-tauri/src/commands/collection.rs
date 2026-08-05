use serde_json::json;
use tauri::State;

use crate::db::repos::tasks::TasksRepo;
use crate::domain::error::ApiError;
use crate::state::AppState;

/// 开始一轮采集：按设置中的配额（每轮条数 / TT 比例 / IG 比例 / 点赞上限）
/// 为 TikTok 与 Instagram 各入队一个 collect_batch 任务。
#[tauri::command]
pub fn start_collection(state: State<AppState>) -> Result<Vec<serde_json::Value>, ApiError> {
    let (batch_size, tt_ratio, ig_ratio, max_likes) = {
        let s = state.settings.read().expect("settings lock poisoned");
        (
            s.batch_size.max(1),
            s.tiktok_ratio.clamp(0.0, 1.0),
            s.instagram_ratio.clamp(0.0, 1.0),
            s.max_likes.max(0),
        )
    };
    let total_ratio = (tt_ratio + ig_ratio).max(f64::EPSILON);
    let tt_count = ((batch_size as f64) * tt_ratio / total_ratio).round() as i64;
    let ig_count = ((batch_size as f64) * ig_ratio / total_ratio).round() as i64;

    let repo = TasksRepo::new(&state.db);
    let mut out = Vec::new();
    for (platform, count) in [("tiktok", tt_count), ("instagram", ig_count)] {
        if count <= 0 {
            continue;
        }
        let task_id = repo.enqueue(
            "collector",
            "collect_batch",
            json!({
                "platform": platform,
                "count": count,
                "maxLikes": max_likes,
            }),
            false,
        )?;
        out.push(json!({ "platform": platform, "count": count, "task_id": task_id }));
    }
    Ok(out)
}
