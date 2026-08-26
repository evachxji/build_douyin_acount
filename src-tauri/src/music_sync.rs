//! 剪映专业版缓存音乐同步（前身为 `scripts/export_jianying_music.py`，内化后已删除）。
//!
//! 从剪映本地缓存提取音乐文件，按「歌名 - 作者」重命名后复制到 `storage/music/`：
//! - 音频来源：`%LOCALAPPDATA%\JianyingPro\User Data\Cache\music`
//! - 歌名映射：解析 ressdk_db 资源数据库（复制副本再读，剪映运行时会锁库）+ downLoadcfg 下载索引
//! - 真实格式按文件头探测（缓存的 .mp3 实际是 M4A）；重名追加序号；同内容（大小+MD5）跳过，幂等可重复跑
//! - 未匹配歌名的文件**不同步**（哈希名没有辨认价值；歌名映射只覆盖收藏过的歌，下载未收藏的歌因此也不会进来）
//!
//! 注意：抖音歌曲 ID 是 19 位数字（超 i64::MAX），JSON 解析成 f64 会丢精度导致
//! md5(id) 对不上 downLoadcfg 的 hex——http_cache 反序列化必须用 `&RawValue` 保留数字原文。

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::domain::error::DomainError;

const MAX_NAME_LEN: usize = 120;
/// 目标目录里计为「已同步音乐」的音频扩展名。
const AUDIO_EXTS: &[&str] = &["m4a", "mp3", "aac", "wav", "flac", "ogg", "wma"];

/// 歌名索引：md5(歌曲ID) → (标题, 作者, 封面URL)。封面是抖音音乐库原图（精确匹配，优于联网搜）。
type SongIndex = HashMap<String, (String, String, Option<String>)>;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncReport {
    pub copied: u32,
    pub skipped: u32,
    /// 未匹配歌名而跳过（未复制）的文件数。
    pub unmatched: u32,
    /// 本次涉及的「目标文件名 → 精确封面URL」（无论复制还是跳过；调用方据此落 music_covers 表）。
    pub cover_urls: Vec<(String, String)>,
}

/// 剪映缓存根目录（`%LOCALAPPDATA%\JianyingPro\User Data\Cache`），未安装剪映为 None。
pub fn jianying_cache_root() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local).join("JianyingPro").join("User Data").join("Cache"))
}

/// 目标目录下音频文件数（设置页「已同步 N 首」口径）。
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
            })
        })
        .collect();
    files.sort_by_key(|f| f.file_name.to_lowercase());
    files
}

/// 主流程：把剪映缓存音乐复制到 dest_dir。缓存目录不存在时返回明确错误。
pub fn sync(cache_root: &Path, dest_dir: &Path) -> Result<SyncReport, DomainError> {
    let music_cache = cache_root.join("music");
    if !music_cache.is_dir() {
        return Err(DomainError::InvalidData(format!(
            "未找到剪映音乐缓存目录: {}",
            music_cache.display()
        )));
    }
    std::fs::create_dir_all(dest_dir)?;

    let song_index = load_song_index(cache_root);
    let mut report = SyncReport { copied: 0, skipped: 0, unmatched: 0, cover_urls: Vec::new() };
    let mut dest_md5_cache: HashMap<PathBuf, (u64, String)> = HashMap::new();

    for (hex_id, filename) in load_download_entries(&music_cache)? {
        let src = music_cache.join(&filename);
        if !src.is_file() || src.extension().and_then(|e| e.to_str()) == Some("beat") {
            continue;
        }
        let ext = detect_ext(&src)?;
        let (base, cover_url) = match song_index.get(&hex_id) {
            Some((title, author, cover)) if !author.is_empty() => {
                (sanitize(&format!("{title} - {author}")), cover.clone())
            }
            Some((title, _, cover)) => (sanitize(title), cover.clone()),
            // 未匹配歌名的不同步（哈希名没有辨认价值，2026-08-26 用户明确不要）
            None => {
                report.unmatched += 1;
                continue;
            }
        };
        let src_size = src.metadata()?.len();
        let src_md5 = md5_file(&src)?;
        let dest = match pick_dest(dest_dir, &base, &ext, src_size, &src_md5, &mut dest_md5_cache)? {
            DestAction::Skip(dest) => {
                report.skipped += 1;
                dest
            }
            DestAction::Copy(dest) => {
                std::fs::copy(&src, &dest)?;
                dest_md5_cache.insert(dest.clone(), (src_size, src_md5));
                report.copied += 1;
                dest
            }
        };
        // 复制/跳过都记录精确封面映射（已同步过的老文件同样要补封面）。
        if let Some(url) = cover_url {
            if let Some(name) = dest.file_name() {
                report
                    .cover_urls
                    .push((name.to_string_lossy().into_owned(), url));
            }
        }
    }
    Ok(report)
}

/// 从剪映资源数据库提取歌曲信息。ressdk_db 缺失/单库损坏不致命（静默跳过 + warn），
/// 返回空索引时所有文件都无法匹配歌名（将全部跳过不同步）。
fn load_song_index(cache_root: &Path) -> SongIndex {
    let mut index = SongIndex::new();
    let ressdk_db = cache_root.join("ressdk_db");
    if !ressdk_db.is_dir() {
        return index;
    }
    let db_files = find_db_files(&ressdk_db);
    let Ok(tmp) = tempfile::tempdir() else {
        tracing::warn!("剪映歌曲库：无法创建临时目录，跳过歌名映射");
        return index;
    };
    for (i, db) in db_files.iter().enumerate() {
        // 剪映运行时数据库被占用，复制副本（含 WAL/SHM）再读
        let dst_dir = tmp.path().join(format!("db{i}"));
        if std::fs::create_dir_all(&dst_dir).is_err() {
            continue;
        }
        let Some(db_name) = db.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Ok(siblings) = std::fs::read_dir(db.parent().unwrap_or(ressdk_db.as_path())) {
            for f in siblings.flatten() {
                let name = f.file_name();
                if name.to_str().map(|n| n.starts_with(db_name)).unwrap_or(false) {
                    let _ = std::fs::copy(f.path(), dst_dir.join(&name));
                }
            }
        }
        match rusqlite::Connection::open(dst_dir.join(db_name)) {
            Ok(con) => collect_from_db(&con, &mut index),
            Err(e) => tracing::warn!("剪映歌曲库 {} 打开失败（跳过）: {e}", db.display()),
        }
    }
    index
}

/// 递归找 ressdk_db 下的 rp.db / rp_master.db。
fn find_db_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() => {
                    if matches!(p.file_name().and_then(|n| n.to_str()), Some("rp.db") | Some("rp_master.db")) {
                        out.push(p);
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn collect_from_db(con: &rusqlite::Connection, index: &mut SongIndex) {
    // 来源1: http_cache 中的接口响应缓存（收藏列表/推荐列表等）
    if let Ok(mut stmt) = con.prepare("SELECT response_body FROM http_cache") {
        let bodies: Vec<Option<String>> = stmt
            .query_map([], |row| row.get(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default();
        for body in bodies.into_iter().flatten() {
            collect_from_http_body(&body, index);
        }
    }
    // 来源2: music 表（部分版本直接落库）。优先带封面列的查询，老库无封面列时退化。
    /// (id, title, author, cover_url_hd, cover_url_medium)
    type MusicRow = (
        rusqlite::types::Value,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let music_sql = "SELECT id, title, author, cover_url_hd, cover_url_medium FROM music";
    let fallback_sql = "SELECT id, title, author, NULL, NULL FROM music";
    let sql = if con.prepare(music_sql).is_ok() { music_sql } else { fallback_sql };
    if let Ok(mut stmt) = con.prepare(sql) {
        let rows: Vec<MusicRow> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default();
        for (id, title, author, hd, medium) in rows {
            add_song(index, sql_id_to_string(&id), title, author, hd.or(medium));
        }
    }
}

#[derive(Deserialize)]
struct HttpCacheBody<'a> {
    #[serde(borrow)]
    data: Option<BodyData<'a>>,
}

#[derive(Deserialize)]
struct BodyData<'a> {
    #[serde(borrow)]
    songs: Option<Vec<SongEntry<'a>>>,
}

#[derive(Deserialize)]
struct SongEntry<'a> {
    #[serde(borrow)]
    id: Option<&'a RawValue>,
    title: Option<String>,
    author: Option<String>,
    /// 封面：多数版本是字符串化的字典（"{'hd': 'https://…'}"），少数是 JSON 对象/裸 URL。
    cover_url: Option<serde_json::Value>,
}

/// 从 cover_url 字段提取可用 URL（优先 hd/large，其次 medium/thumb/裸串）。
fn extract_cover_url(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => {
            let s = s.trim();
            if s.starts_with("http") {
                return Some(s.to_string());
            }
            // 字符串化字典：抽第一个 http(s) URL（到引号/空格/右括号为止）。
            let start = s.find("http")?;
            let rest = &s[start..];
            let end = rest
                .find(|c| ['\'', '"', ' ', '}'].contains(&c))
                .unwrap_or(rest.len());
            let url = rest[..end].trim();
            (!url.is_empty()).then(|| url.to_string())
        }
        serde_json::Value::Object(map) => ["hd", "large", "medium", "thumb"]
            .iter()
            .find_map(|k| map.get(*k)?.as_str().map(str::to_string)),
        _ => None,
    }
}

fn collect_from_http_body(body: &str, index: &mut SongIndex) {
    let Ok(parsed) = serde_json::from_str::<HttpCacheBody>(body) else {
        return;
    };
    let Some(songs) = parsed.data.and_then(|d| d.songs) else {
        return;
    };
    for s in songs {
        add_song(
            index,
            s.id.and_then(raw_id_to_string),
            s.title,
            s.author,
            s.cover_url.as_ref().and_then(extract_cover_url),
        );
    }
}

fn add_song(
    index: &mut SongIndex,
    song_id: Option<String>,
    title: Option<String>,
    author: Option<String>,
    cover_url: Option<String>,
) {
    if let (Some(id), Some(title)) = (song_id, title) {
        if !id.is_empty() && !title.is_empty() {
            // setdefault：先来的赢（与 Python 版一致）
            index
                .entry(md5_hex(id.as_bytes()))
                .or_insert((title, author.unwrap_or_default(), cover_url));
        }
    }
}

/// 歌曲 ID → 精确十进制原文。19 位 ID 超 i64::MAX，必须保留 RawValue 原文；
/// 字符串形式则剥引号取内容（Python `str(song_id)` 两种形态输出一致）。
fn raw_id_to_string(raw: &RawValue) -> Option<String> {
    let s = raw.get().trim();
    if s.is_empty() || s == "null" {
        return None;
    }
    if s.starts_with('"') {
        serde_json::from_str::<String>(s).ok().filter(|v| !v.is_empty())
    } else {
        Some(s.to_string())
    }
}

fn sql_id_to_string(v: &rusqlite::types::Value) -> Option<String> {
    use rusqlite::types::Value;
    match v {
        Value::Integer(i) => Some(i.to_string()),
        Value::Text(s) if !s.is_empty() => Some(s.clone()),
        _ => None,
    }
}

#[derive(Deserialize)]
struct DownloadCfg {
    list: Vec<DownloadEntry>,
}

#[derive(Deserialize)]
struct DownloadEntry {
    hex: String,
    path: String,
}

/// 解析 downLoadcfg 下载索引 → [(hex, 文件名)]；索引缺失时退化为扫描目录（hex 为空，
/// 全部未匹配歌名而跳过，实际不复制任何文件）。cfg 存在但解析失败是明确错误。
fn load_download_entries(music_cache: &Path) -> Result<Vec<(String, String)>, DomainError> {
    let cfg = music_cache.join("downLoadcfg");
    if cfg.is_file() {
        let text = std::fs::read_to_string(&cfg)?;
        let parsed: DownloadCfg = serde_json::from_str(&text).map_err(|e| {
            DomainError::InvalidData(format!("剪映 downLoadcfg 解析失败: {e}"))
        })?;
        return Ok(parsed.list.into_iter().map(|e| (e.hex, e.path)).collect());
    }
    let mut names: Vec<String> = std::fs::read_dir(music_cache)?
        .flatten()
        .filter(|e| {
            e.file_type().map(|t| t.is_file()).unwrap_or(false)
                && e.file_name().to_str().map(|n| {
                    n != "downLoadcfg" && !n.to_lowercase().ends_with(".beat")
                }).unwrap_or(false)
        })
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    Ok(names.into_iter().map(|n| (String::new(), n)).collect())
}

/// 按文件头探测真实格式：剪映缓存的 .mp3 实际是 M4A(AAC)。
fn detect_ext(path: &Path) -> Result<String, DomainError> {
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
fn sanitize(name: &str) -> String {
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

enum DestAction {
    Copy(PathBuf),
    /// 同内容已存在（携带已存在的目标路径，封面映射要用文件名）。
    Skip(PathBuf),
}

/// 重名处理：同名同内容（大小+MD5）→ Skip；同名不同内容 → 追加序号 (2)(3)…
fn pick_dest(
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

fn md5_hex(bytes: &[u8]) -> String {
    format!("{:x}", Md5::digest(bytes))
}

fn md5_file(path: &Path) -> Result<String, DomainError> {
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
