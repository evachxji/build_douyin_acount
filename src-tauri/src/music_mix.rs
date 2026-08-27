//! 视频+音乐合成（详情页「添加音乐」核心）。
//!
//! 合成语义：
//! - 视频流 copy 不重编码（秒级完成）；音频统一重编码 AAC 192k。
//! - 音乐用 `-t <视频时长>` 裁到视频长度（音乐更短时尾部自然静音，不做循环）。
//! - 「关闭原声」或视频本身无音轨：音乐按音乐音量滑条铺底（原声滑条不生效）；
//!   否则原声与音乐按用户滑条音量（百分比，默认 30%/70%）混音，alimiter 防叠加削波。
//! - 再次合成从原始视频重来（`music_info.origin_file_path`），不会叠音；
//!   输出固定 `videos/<原stem>.music.mp4` 幂等覆盖。

use std::path::{Path, PathBuf};

use crate::db::repos::assets::AssetsRepo;
use crate::db::Database;
use crate::domain::asset::{AssetStatus, MusicInfo};
use crate::domain::error::DomainError;
use crate::media::{probe_duration_ms, MediaStore};
use crate::util::run_tool;

/// 合成结果（命令层透传前端）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct MusicApplyResult {
    /// 合成产物路径（素材 file_path 已指向它）。
    pub file_path: String,
    pub music_info: MusicInfo,
}

/// ffmpeg 执行器签名（生产 = `ffmpeg_exec`；测试 = 假 ffmpeg 写占位文件）。
pub type MergeExec<'a> = dyn Fn(&str, &[String]) -> Result<Vec<u8>, DomainError> + 'a;

/// 生产执行器：经 run_tool 调内置 ffmpeg。
pub fn ffmpeg_exec(name: &str, args: &[String]) -> Result<Vec<u8>, DomainError> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_tool(name, &refs)
}

/// 探测视频是否有音频流（决定混音还是直接铺音乐）。
fn has_audio_stream(path: &Path) -> Result<bool, DomainError> {
    let out = run_tool(
        "ffprobe",
        &[
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
            path.to_string_lossy().as_ref(),
        ],
    )?;
    Ok(!String::from_utf8_lossy(&out).trim().is_empty())
}

/// 混音选项（对应弹窗的「关闭原声」开关与两个音量滑条）。
#[derive(Debug, Clone, Copy)]
pub struct MixOptions {
    /// 关闭原声（源视频无音轨时同效退化）：仅音乐铺底，原声音量滑条不生效。
    pub mute: bool,
    /// 原声音量百分比 0-100（保留原声混音时生效）。
    pub video_volume: i64,
    /// 音乐音量百分比 0-100（混音与铺底都生效）。
    pub music_volume: i64,
}

/// 音量百分比 → ffmpeg volume 比率（0-100 钳制后 /100）。
fn volume_ratio(percent: i64) -> f64 {
    percent.clamp(0, 100) as f64 / 100.0
}

/// 合成 ffmpeg 参数（纯函数，测试友好）。
/// `duration_s` 为源视频时长：`-t` 把音乐裁到该长度。
pub fn build_merge_args(
    video: &Path,
    music: &Path,
    out: &Path,
    duration_s: f64,
    video_has_audio: bool,
    opts: MixOptions,
) -> Vec<String> {
    let v = video.to_string_lossy().into_owned();
    let m = music.to_string_lossy().into_owned();
    let o = out.to_string_lossy().into_owned();
    let mut args = vec![
        "-y".to_string(),
        "-i".to_string(),
        v,
        "-i".to_string(),
        m,
    ];
    if opts.mute || !video_has_audio {
        // 关原声（或源无音轨）：仅音乐按滑条音量铺底（音频本来就要重编码 AAC，滤镜无额外代价）。
        args.extend([
            "-filter_complex".to_string(),
            format!("[1:a]volume={:.2}[m]", volume_ratio(opts.music_volume)),
        ]);
        args.extend(["-map".to_string(), "0:v:0".to_string()]);
        args.extend(["-map".to_string(), "[m]".to_string()]);
    } else {
        // 保留原声：两路各按滑条音量混音，alimiter 防叠加削波。
        args.extend([
            "-filter_complex".to_string(),
            format!(
                "[0:a]volume={:.2}[a0];[1:a]volume={:.2}[m];[a0][m]amix=inputs=2:duration=first:dropout_transition=0:normalize=0,alimiter=limit=0.95[a]",
                volume_ratio(opts.video_volume),
                volume_ratio(opts.music_volume)
            ),
        ]);
        args.extend(["-map".to_string(), "0:v:0".to_string()]);
        args.extend(["-map".to_string(), "[a]".to_string()]);
    }
    args.extend([
        "-c:v".to_string(),
        "copy".to_string(),
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        "192k".to_string(),
        "-t".to_string(),
        format!("{duration_s:.3}"),
        "-movflags".to_string(),
        "+faststart".to_string(),
        o,
    ]);
    args
}

/// 把音乐合成进素材视频并更新 file_path/music_info。
/// `exec` 注入执行器（生产 = `ffmpeg_exec`；测试 = 假 ffmpeg 写占位文件）。
pub fn apply_music(
    db: &Database,
    store: &MediaStore,
    asset_id: i64,
    music_path: &Path,
    opts: MixOptions,
    exec: &MergeExec<'_>,
) -> Result<MusicApplyResult, DomainError> {
    let repo = AssetsRepo::new(db);
    let asset = repo
        .get(asset_id)?
        .ok_or(DomainError::AssetNotFound(asset_id))?;
    if !matches!(asset.status, AssetStatus::Candidate | AssetStatus::Selected) {
        return Err(DomainError::InvalidData(format!(
            "仅候选/待发布素材可添加音乐（当前状态 {}）",
            asset.status
        )));
    }
    // 音乐必须来自音乐库目录（防任意路径注入）。
    let music_dir = store.music_dir().canonicalize()?;
    let music_abs = music_path
        .canonicalize()
        .map_err(|_| DomainError::InvalidData(format!("音乐文件不存在: {}", music_path.display())))?;
    if !music_abs.starts_with(&music_dir) {
        return Err(DomainError::InvalidData(
            "音乐文件必须来自音乐库目录（storage/music）".to_string(),
        ));
    }
    // 再次合成从原始视频重来（不叠音）；首次合成源即当前 file_path。
    let (source, origin_file_path) = match &asset.music_info {
        Some(info) => (PathBuf::from(&info.origin_file_path), info.origin_file_path.clone()),
        None => (PathBuf::from(&asset.file_path), asset.file_path.clone()),
    };
    if !source.is_file() {
        return Err(DomainError::InvalidData(format!(
            "源视频文件不存在: {}",
            source.display()
        )));
    }
    let duration_s = probe_duration_ms(&source)? as f64 / 1000.0;
    // 关原声时无需探测音轨（直接铺音乐）。
    let video_has_audio = !opts.mute && has_audio_stream(&source)?;
    let stem = source
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| DomainError::InvalidData(format!("源视频路径异常: {}", source.display())))?;
    let out = store.videos_dir().join(format!("{stem}.music.mp4"));
    let args = build_merge_args(&source, &music_abs, &out, duration_s, video_has_audio, opts);
    exec("ffmpeg", &args)?;
    if !out.is_file() {
        return Err(DomainError::InvalidData(
            "合成失败：ffmpeg 未生成输出文件".to_string(),
        ));
    }
    let music_info = MusicInfo {
        origin_file_path,
        music_name: music_abs
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        mute_original: opts.mute,
        video_volume: opts.video_volume,
        music_volume: opts.music_volume,
    };
    let out_str = out.to_string_lossy().into_owned();
    repo.apply_music(asset_id, &out_str, &serde_json::to_string(&music_info)?)?;
    Ok(MusicApplyResult {
        file_path: out_str,
        music_info,
    })
}
