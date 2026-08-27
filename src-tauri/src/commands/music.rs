use serde::Serialize;
use tauri::{Manager, State};

use crate::domain::error::{ApiError, DomainError};
use crate::music_mix::{self, MusicApplyResult};
use crate::music_sync::{self, MusicFile};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct MusicStatus {
    /// 音乐库目录里的音频文件数（「曲库 N 首」口径）。
    pub count: u32,
    /// 本地音乐目录绝对路径（storage/music）。
    pub dir: String,
}

fn status(store: &crate::media::MediaStore) -> MusicStatus {
    let dir = store.music_dir();
    MusicStatus {
        count: music_sync::count_music_files(&dir),
        dir: dir.to_string_lossy().into_owned(),
    }
}

/// 音乐库本地状态（设置页打开时调用；扫目录计数，毫秒级）。
#[tauri::command]
pub fn get_music_status(state: State<AppState>) -> MusicStatus {
    status(&state.store)
}

/// 列出音乐库全部音频文件（添加音乐弹窗的数据源），并联出已匹配的专辑封面与所属分类
/// （分类来自 music_categories 表——音乐库采集落库；无分类的歌（如手动放入目录的）弹窗归「未分类」）。
#[tauri::command]
pub fn list_music_files(state: State<AppState>) -> Vec<MusicFile> {
    let mut files = music_sync::list_music(&state.store.music_dir());
    if let Ok(covers) = crate::music_covers::cover_map(&state.db) {
        for f in &mut files {
            f.cover_path = covers.get(&f.file_name).cloned();
        }
    }
    if let Ok(cats) = crate::music_collect::categories_map(&state.db) {
        for f in &mut files {
            if let Some((v, dur)) = cats.get(&f.file_name) {
                f.categories = v.clone();
                f.duration_s = *dur;
            }
        }
    }
    files
}

/// 为缺封面的音乐增量匹配专辑封面（iTunes Search API，免 key）。
/// 增量：已有封面/7 天内查过未找到的跳过，只查新增或缺失的几首，不拖慢采集。
#[tauri::command]
pub async fn ensure_music_covers(
    state: State<'_, AppState>,
) -> Result<crate::music_covers::CoverReport, ApiError> {
    crate::music_covers::ensure_covers(&state.db, &state.store)
        .await
        .map_err(ApiError::from)
}

/// 把选中的音乐合成进素材视频（候选/待发布）。合成是秒级~十秒级 ffmpeg 调用，
/// 走 spawn_blocking 不占主线程，前端按钮 loading 等待结果即可（无需进度轮询）。
/// `video_volume` 为保留原声时的原声增益百分比（0-100）；`music_volume` 为音乐增益（混音/铺底都生效）。
#[tauri::command]
pub async fn add_music_to_video(
    app: tauri::AppHandle,
    asset_id: i64,
    music_path: String,
    mute_original: bool,
    video_volume: i64,
    music_volume: i64,
) -> Result<MusicApplyResult, ApiError> {
    let state = app.state::<AppState>();
    let db = state.db.clone();
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        music_mix::apply_music(
            &db,
            &store,
            asset_id,
            std::path::Path::new(&music_path),
            music_mix::MixOptions {
                mute: mute_original,
                video_volume,
                music_volume,
            },
            &music_mix::ffmpeg_exec,
        )
    })
    .await
    .unwrap_or_else(|e| Err(DomainError::InvalidData(format!("合成线程异常: {e}"))))
    .map_err(ApiError::from)
}

/// 在系统文件管理器中打开本地音乐目录。
#[tauri::command]
pub fn open_music_dir(state: State<AppState>) -> Result<(), ApiError> {
    let dir = state.store.music_dir();
    std::fs::create_dir_all(&dir).map_err(DomainError::from)?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(DomainError::from)?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(&dir)
        .spawn()
        .map_err(DomainError::from)?;
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    std::process::Command::new("xdg-open")
        .arg(&dir)
        .spawn()
        .map_err(DomainError::from)?;
    Ok(())
}
