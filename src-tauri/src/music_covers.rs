//! 音乐专辑封面匹配：为音乐库每首音乐抓取封面，三级回退——
//! 1. 音乐库采集拿到的精确封面（抖音音乐库原图，1080×1080，精确匹配）；
//! 2. QQ 音乐搜索（免登录公开接口，中文歌覆盖最好）；
//! 3. iTunes Search API（免 key，兜底）。
//!
//! - 封面落盘 `storage/music/covers/<md5(file_name)>.jpg`（md5 避开文件名特殊字符/超长问题），
//!   「音乐 ↔ 封面」映射存 `music_covers` 表（file_name 为主键）。
//! - 增量：FOUND 且封面文件存在 → 跳过；NOT_FOUND 自 queried_at 起 7 天内不重试——
//!   每次同步只查新增/缺失的几首，不重下全量。
//! - 单条失败（超时/未命中/写盘错）只记 NOT_FOUND，不中断整体。

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use md5::{Digest, Md5};
use serde::Serialize;

use crate::db::Database;
use crate::domain::error::DomainError;
use crate::media::MediaStore;
use crate::music_sync;

/// iTunes Search API（免 key，兜底源）。
const ITUNES_API: &str = "https://itunes.apple.com/search";
/// QQ 音乐搜索接口（免登录；中文歌覆盖最好的回退源）。
const QQ_SEARCH_API: &str = "https://c.y.qq.com/soso/fcgi-bin/client_search_cp";
/// 单条网络请求超时（搜索 + 下载各一次）。
const HTTP_TIMEOUT: Duration = Duration::from_secs(8);
/// NOT_FOUND 记录的重试间隔：失败项每周最多重查一次，避免每次同步重复打外网。
const RETRY_NOT_FOUND_AFTER: &str = "-7 days";

#[derive(Debug, Clone, Serialize)]
pub struct CoverReport {
    /// 本次新下载到封面的数量。
    pub downloaded: u32,
    /// 本次查询后仍未找到/下载失败的数量。
    pub not_found: u32,
    /// 已有封面（或 7 天内已查过未找到）而跳过的数量。
    pub skipped: u32,
}

/// 表记录（对应 music_covers 一行）。
struct CoverRow {
    file_name: String,
    cover_path: Option<String>,
    /// 同步时从剪映缓存提取的精确封面 URL（可能尚未下载）。
    cover_url: Option<String>,
    status: String,
}

/// 从文件名（去扩展名）拆搜索词：「歌名 - 作者」→「歌名 作者」；无作者就只用歌名。
fn search_term(stem: &str) -> String {
    match stem.split_once(" - ") {
        Some((title, artist)) if !title.trim().is_empty() => {
            format!("{} {}", title.trim(), artist.trim())
        }
        _ => stem.trim().to_string(),
    }
}

/// iTunes  artworkUrl100 → 600x600 高清图（同 CDN 路径只换尺寸段，拿不到高清时回退原 URL）。
fn hi_res_artwork(url: &str) -> String {
    url.replace("100x100bb.jpg", "600x600bb.jpg")
}

/// 封面文件名：md5(file_name) + .jpg。
fn cover_file_name(file_name: &str) -> String {
    format!("{:x}.jpg", Md5::digest(file_name.as_bytes()))
}

fn http_client() -> Result<reqwest::Client, DomainError> {
    reqwest::Client::builder()
        .user_agent("build_douyin_acount")
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| DomainError::InvalidData(format!("封面匹配初始化网络客户端失败: {e}")))
}

fn load_rows(db: &Database) -> Result<HashMap<String, CoverRow>, DomainError> {
    let conn = db.conn();
    let mut stmt = conn.prepare("SELECT file_name, cover_path, cover_url, status FROM music_covers")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(CoverRow {
                file_name: r.get(0)?,
                cover_path: r.get(1)?,
                cover_url: r.get(2)?,
                status: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().map(|r| (r.file_name.clone(), r)).collect())
}

/// 记录匹配结果（FOUND/NOT_FOUND）；已有的精确 cover_url 保留不覆盖，供下次重试直接用。
fn upsert_row(
    db: &Database,
    file_name: &str,
    cover_path: Option<&str>,
    status: &str,
) -> Result<(), DomainError> {
    db.conn().execute(
        "INSERT INTO music_covers (file_name, cover_path, status, queried_at)
         VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(file_name) DO UPDATE SET
           cover_path = excluded.cover_path,
           status = excluded.status,
           queried_at = excluded.queried_at",
        rusqlite::params![file_name, cover_path, status],
    )?;
    Ok(())
}

/// 采集时写入精确封面 URL（抖音音乐库原图）。
/// 已有 FOUND 的行不动（避免重复下载）；其余一律置为 URL_READY 等 ensure_covers 下载。
pub fn upsert_exact_url(db: &Database, file_name: &str, url: &str) -> Result<(), DomainError> {
    db.conn().execute(
        "INSERT INTO music_covers (file_name, cover_url, status, queried_at)
         VALUES (?1, ?2, 'URL_READY', datetime('now'))
         ON CONFLICT(file_name) DO UPDATE SET
           cover_url = excluded.cover_url,
           status = 'URL_READY',
           queried_at = excluded.queried_at
         WHERE music_covers.status != 'FOUND'",
        rusqlite::params![file_name, url],
    )?;
    Ok(())
}

/// 下载图片到 covers_dir，返回本地路径。
async fn download_image(
    client: &reqwest::Client,
    url: &str,
    covers_dir: &Path,
    file_name: &str,
) -> Option<String> {
    let bytes = client
        .get(url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .bytes()
        .await
        .ok()?;
    // 防挂掉图床返回错误页：图片不会小于 1KB。
    if bytes.len() < 1024 {
        return None;
    }
    let path = covers_dir.join(cover_file_name(file_name));
    std::fs::write(&path, &bytes).ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// QQ 音乐搜索 → 封面 URL（albummid 拼 y.gtimg.cn 图床直链）。
async fn qq_cover_url(client: &reqwest::Client, term: &str) -> Option<String> {
    let resp: serde_json::Value = client
        .get(QQ_SEARCH_API)
        .query(&[("w", term), ("format", "json"), ("n", "1"), ("t", "0")])
        .header(reqwest::header::REFERER, "https://y.qq.com/")
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let mid = resp
        .get("data")?
        .get("song")?
        .get("list")?
        .get(0)?
        .get("albummid")?
        .as_str()?;
    (!mid.is_empty()).then(|| {
        format!("https://y.gtimg.cn/music/photo_new/T002R300x300M000{mid}.jpg")
    })
}

/// iTunes 搜索 → 封面 URL（高清化）。
async fn itunes_cover_url(client: &reqwest::Client, term: &str) -> Option<String> {
    let resp: serde_json::Value = client
        .get(ITUNES_API)
        .query(&[
            ("term", term),
            ("media", "music"),
            ("entity", "song"),
            ("limit", "1"),
            ("country", "CN"),
        ])
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    let artwork = resp
        .get("results")?
        .get(0)?
        .get("artworkUrl100")?
        .as_str()?;
    Some(hi_res_artwork(artwork))
}

/// 一首歌的封面解析链：精确 URL（剪映缓存）→ QQ 音乐 → iTunes；命中即下载落盘。
async fn resolve_cover(
    client: &reqwest::Client,
    stem: &str,
    covers_dir: &Path,
    file_name: &str,
    exact_url: Option<&str>,
) -> Option<String> {
    if let Some(url) = exact_url {
        if let Some(p) = download_image(client, url, covers_dir, file_name).await {
            return Some(p);
        }
    }
    let term = search_term(stem);
    if term.is_empty() {
        return None;
    }
    for search in [qq_cover_url(client, &term).await, itunes_cover_url(client, &term).await]
        .into_iter()
        .flatten()
    {
        if let Some(p) = download_image(client, &search, covers_dir, file_name).await {
            return Some(p);
        }
    }
    None
}

/// 扫描音乐目录，为缺封面的音乐增量匹配专辑封面（同步后/打开添加音乐弹窗时调用）。
pub async fn ensure_covers(db: &Database, store: &MediaStore) -> Result<CoverReport, DomainError> {
    let files = music_sync::list_music(&store.music_dir());
    let rows = load_rows(db)?;

    // 清理：音乐文件已删除的行 → 删行 + 删封面文件（映射不腐烂）。
    {
        let conn = db.conn();
        for row in rows.values() {
            if files.iter().any(|f| f.file_name == row.file_name) {
                continue;
            }
            if let Some(p) = &row.cover_path {
                let _ = std::fs::remove_file(p);
            }
            conn.execute(
                "DELETE FROM music_covers WHERE file_name = ?1",
                rusqlite::params![row.file_name],
            )?;
        }
    }

    // 差集出需要（重）查询/下载的文件。
    let mut pending = Vec::new();
    let mut skipped: u32 = 0;
    for f in &files {
        let need = match rows.get(&f.file_name) {
            None => true,
            Some(row) => match row.status.as_str() {
                // 精确 URL 待下载：必处理。
                "URL_READY" => true,
                // FOUND 但封面文件被删：重新匹配。
                "FOUND" => row
                    .cover_path
                    .as_deref()
                    .map(|p| !Path::new(p).is_file())
                    .unwrap_or(true),
                // NOT_FOUND：7 天内不重试（queried_at 超过窗口才重查）。
                _ => db.conn().query_row(
                    "SELECT queried_at < datetime('now', ?1) FROM music_covers WHERE file_name = ?2",
                    rusqlite::params![RETRY_NOT_FOUND_AFTER, f.file_name],
                    |r| r.get::<_, bool>(0),
                )?,
            },
        };
        if need {
            pending.push(f);
        } else {
            skipped += 1;
        }
    }

    let mut report = CoverReport {
        downloaded: 0,
        not_found: 0,
        skipped,
    };
    if pending.is_empty() {
        return Ok(report);
    }

    std::fs::create_dir_all(store.music_covers_dir())?;
    let client = http_client()?;
    for f in pending {
        let exact = rows.get(&f.file_name).and_then(|r| r.cover_url.as_deref());
        match resolve_cover(&client, &f.name, &store.music_covers_dir(), &f.file_name, exact).await {
            Some(path) => {
                upsert_row(db, &f.file_name, Some(&path), "FOUND")?;
                report.downloaded += 1;
            }
            None => {
                upsert_row(db, &f.file_name, None, "NOT_FOUND")?;
                report.not_found += 1;
            }
        }
    }
    Ok(report)
}

/// file_name → 封面路径映射（list_music_files 联封面用；只取 FOUND 且文件存在的）。
pub fn cover_map(db: &Database) -> Result<HashMap<String, String>, DomainError> {
    Ok(load_rows(db)?
        .into_iter()
        .filter_map(|(name, row)| {
            let p = row.cover_path?;
            (row.status == "FOUND" && Path::new(&p).is_file()).then_some((name, p))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_term_splits_title_and_artist() {
        assert_eq!(search_term("晴天 - 周杰伦"), "晴天 周杰伦");
        assert_eq!(search_term("七里香"), "七里香");
        // 多个「 - 」只拆第一个，作者段原样保留
        assert_eq!(search_term("A - B - C"), "A B - C");
        assert_eq!(search_term(" - 作者"), "- 作者");
    }

    #[test]
    fn hi_res_artwork_upgrades_size() {
        assert_eq!(
            hi_res_artwork("https://is1-ssl.mzstatic.com/image/thumb/Music/100x100bb.jpg"),
            "https://is1-ssl.mzstatic.com/image/thumb/Music/600x600bb.jpg"
        );
        // 非标准 URL 原样返回
        assert_eq!(hi_res_artwork("https://x/y.png"), "https://x/y.png");
    }

    #[test]
    fn cover_file_name_is_md5_jpg() {
        let n = cover_file_name("晴天 - 周杰伦.m4a");
        assert!(n.ends_with(".jpg") && n.len() == 32 + 4);
    }
}
