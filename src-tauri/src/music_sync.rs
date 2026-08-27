//! 音乐库基础工具：`storage/music/` 目录扫描、音频格式探测、文件命名与去重规则。
//!
//! （原为剪映专业版缓存音乐同步；2026-08-27 该功能已由「音乐库采集」（music_collect，
//! 直接走剪映线上音乐库 API）整体取代，同步逻辑全部删除，本模块只留共用基础：
//! `list_music`/`count_music_files` 供添加音乐弹窗与封面匹配，`sanitize`/`detect_ext`/
//! `pick_dest`/`md5_file` 供采集落盘。）

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};

use crate::domain::error::DomainError;

const MAX_NAME_LEN: usize = 120;
/// 目标目录里计为「已同步音乐」的音频扩展名。
const AUDIO_EXTS: &[&str] = &["m4a", "mp3", "aac", "wav", "flac", "ogg", "wma"];

/// 目标目录下音频文件数（设置页「曲库 N 首」口径）。
pub fn count_music_files(dir: &Path) -> u32 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            e.file_type().map(|t| t.is_file()).unwrap_or(false)
                && e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|x| AUDIO_EXTS.contains(&x.to_lowercase().as_str()))
                    .unwrap_or(false)
        })
        .count() as u32
}

/// 音乐库文件（添加音乐弹窗列表项）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct MusicFile {
    /// 文件名去扩展名（展示用）。
    pub name: String,
    /// 完整文件名（合成后记录到素材的 music_name）。
    pub file_name: String,
    /// 绝对路径。
    pub path: String,
    pub size_bytes: u64,
    /// 专辑封面绝对路径（music_covers 表联出；未匹配到为 None）。
    pub cover_path: Option<String>,
    /// 所属分类（music_categories 表联出，按剪映栏目顺序；无映射为空 = 「未分类」）。
    pub categories: Vec<String>,
}

/// 列出音乐目录下全部音频文件（按文件名排序，目录不存在/为空返回空列表）。
pub fn list_music(dir: &Path) -> Vec<MusicFile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<MusicFile> = entries
        .flatten()
        .filter(|e| {
            e.file_type().map(|t| t.is_file()).unwrap_or(false)
                && e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|x| AUDIO_EXTS.contains(&x.to_lowercase().as_str()))
                    .unwrap_or(false)
        })
        .filter_map(|e| {
            let p = e.path();
            let meta = e.metadata().ok()?;
            Some(MusicFile {
                name: p.file_stem().map(|s| s.to_string_lossy().into_owned())?,
                file_name: p.file_name().map(|s| s.to_string_lossy().into_owned())?,
                path: p.to_string_lossy().into_owned(),
                size_bytes: meta.len(),
                cover_path: None,
                categories: Vec::new(),
            })
        })
        .collect();
    files.sort_by_key(|f| f.file_name.to_lowercase());
    files
}

/// 按文件头探测真实格式（剪映/抖音缓存的 .mp3 实际多是 M4A(AAC)）。
pub(crate) fn detect_ext(path: &Path) -> Result<String, DomainError> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = [0u8; 12];
    let n = f.read(&mut buf)?;
    let head = &buf[..n];
    if head.len() >= 8 && &head[4..8] == b"ftyp" {
        return Ok(".m4a".to_string());
    }
    if head.starts_with(b"ID3") || (head.len() >= 2 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0) {
        return Ok(".mp3".to_string());
    }
    Ok(path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_else(|| ".bin".to_string()))
}

/// 文件名清洗：非法字符 → `_`，去两端空格与点，截 120 字符，空名兜底 untitled。
pub(crate) fn sanitize(name: &str) -> String {
    const INVALID: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
    let cleaned: String = name
        .chars()
        .map(|c| if INVALID.contains(&c) { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim_matches(|c| c == ' ' || c == '.');
    let truncated: String = trimmed.chars().take(MAX_NAME_LEN).collect();
    if truncated.is_empty() {
        "untitled".to_string()
    } else {
        truncated
    }
}

pub(crate) enum DestAction {
    Copy(PathBuf),
    /// 同内容已存在（携带已存在的目标路径，封面映射要用文件名）。
    Skip(PathBuf),
}

/// 重名处理：同名同内容（大小+MD5）→ Skip；同名不同内容 → 追加序号 (2)(3)…
pub(crate) fn pick_dest(
    dest_dir: &Path,
    base: &str,
    ext: &str,
    src_size: u64,
    src_md5: &str,
    cache: &mut HashMap<PathBuf, (u64, String)>,
) -> Result<DestAction, DomainError> {
    for n in 1..1000u32 {
        let name = if n == 1 {
            format!("{base}{ext}")
        } else {
            format!("{base} ({n}){ext}")
        };
        let dest = dest_dir.join(name);
        if !dest.exists() {
            return Ok(DestAction::Copy(dest));
        }
        let size = dest.metadata()?.len();
        let md5 = match cache.get(&dest) {
            Some(v) => v.clone(),
            None => {
                let v = (size, md5_file(&dest)?);
                cache.insert(dest.clone(), v.clone());
                v
            }
        };
        if md5.0 == src_size && md5.1 == src_md5 {
            return Ok(DestAction::Skip(dest));
        }
    }
    Err(DomainError::InvalidData(format!("重名冲突过多: {base}")))
}

pub(crate) fn md5_file(path: &Path) -> Result<String, DomainError> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Md5::new();
    // 栈上缓冲只能给 64KB：Windows 主线程栈默认 1MB，[0u8; 1<<20] 直接栈溢出
    // （集成测试线程栈 2MB 测不出来，真实 app 主线程必炸）。
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
