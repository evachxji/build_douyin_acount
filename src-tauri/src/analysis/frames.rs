use std::path::{Path, PathBuf};

use crate::domain::error::DomainError;
use crate::media::probe_duration_ms;
use crate::util::run_tool;

use super::pipeline::FrameExtractor;

/// 依据视频时长决定抽帧数量：3~6 帧，时长越长帧数越多。
pub fn frame_count_for_duration(duration_ms: i64) -> usize {
    let secs = duration_ms.max(0) as f64 / 1000.0;
    let count = match secs {
        s if s < 15.0 => 3,
        s if s < 45.0 => 4,
        s if s < 90.0 => 5,
        _ => 6,
    };
    count.clamp(3, 6)
}

/// 帧提取器：使用 FFmpeg 按均匀间隔抽帧，并可提取 16kHz 单声道音频。
pub struct FfmpegFrameExtractor;

impl FrameExtractor for FfmpegFrameExtractor {
    fn extract(&self, video: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, DomainError> {
        std::fs::create_dir_all(out_dir)?;
        let duration_ms = probe_duration_ms(video)?;
        let count = frame_count_for_duration(duration_ms);
        let secs = (duration_ms as f64 / 1000.0).max(0.1);
        // fps = count/duration，使采样点均匀分布在整个视频中。
        let fps = (count as f64 / secs).max(0.05);
        let pattern = out_dir.join("frame_%03d.jpg");
        run_tool(
            "ffmpeg",
            &[
                "-y",
                "-i",
                video.to_string_lossy().as_ref(),
                "-vf",
                // 抽帧均匀分布 + 长边限制 1280：控制上传体积与云端视觉 token 消耗，
                // 本地模型处理小图也更快（单引号是 ffmpeg filter 语法，非 shell 引用）。
                &format!("fps={fps:.3},scale='min(iw,1280)':'min(ih,1280)':force_original_aspect_ratio=decrease"),
                "-q:v",
                "2",
                pattern.to_string_lossy().as_ref(),
            ],
        )?;
        let mut files: Vec<PathBuf> = std::fs::read_dir(out_dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("jpg"))
            })
            .collect();
        files.sort();
        files.truncate(count);
        if files.is_empty() {
            return Err(DomainError::Process {
                cmd: "ffmpeg".to_string(),
                detail: "frame extraction produced no output".to_string(),
            });
        }
        Ok(files)
    }

    fn extract_audio(&self, video: &Path, out_dir: &Path) -> Result<PathBuf, DomainError> {
        std::fs::create_dir_all(out_dir)?;
        let wav = out_dir.join("audio.wav");
        run_tool(
            "ffmpeg",
            &[
                "-y",
                "-i",
                video.to_string_lossy().as_ref(),
                "-vn",
                "-ac",
                "1",
                "-ar",
                "16000",
                wav.to_string_lossy().as_ref(),
            ],
        )?;
        Ok(wav)
    }
}
