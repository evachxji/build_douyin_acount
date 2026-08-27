//! 剪映音乐库批量采集（2026-08-27 新增）。
//!
//! 直接调剪映音乐库线上 API 按分类批量下载音频到 `storage/music/`，不依赖剪映客户端：
//! - 分类列表：`POST {base}/lv/v1/get_collections`（31 个分类，即剪映「音乐库」左侧栏目）
//! - 分类歌曲：`POST {base}/lv/v1/get_collection_songs`（body `{"id":<分类id>,"cursor":0,"count":50}`，
//!   `has_more`+`next_offset` 翻页；count=50 实测只回 48，须翻页补足）
//! - 音频下载：歌曲 `preview_url`（签名 CDN URL，现拉现下不持久化），实测为 m4a(AAC ~128kbps)
//!
//! 端点与公共 query 参数逆向自剪映专业版 11.3.0.14362（VEConfig.dll，2026-08-27 裸请求实测可用，
//! 探测脚本 storage/logs/try_jy_api*.py）。**分类/歌曲 id 是 19 位数字：u64 收安全（< u64::MAX），
//! 严禁经 f64 中转（丢精度）**；映射表一律存 TEXT。
//!
//! 线程模型与 whisper 模型下载同款：专用线程 + progress.json（tmp+rename，前端轮询；
//! 非终态 mtime>10s 判 interrupted）+ AtomicBool 取消。幂等：song_id 已在 music_categories
//! 且文件在 → 只补分类映射不重复下载；重跑全 skip 秒回。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use rusqlite::OptionalExtension as _;

use crate::db::Database;
use crate::domain::error::DomainError;
use crate::media::MediaStore;
use crate::music_covers;
use crate::music_sync;

/// 线上 API 根（测试经参数注入 mock server 地址替换）。
pub const API_BASE: &str = "https://lv-pc-api.ulikecam.com";
/// 公共 query 参数（逆向自剪映 11.3.0.14362；aid=3704 是剪映 PC 的固定 app id）。
const COMMON_QUERY: &str = "aid=3704&device_platform=windows&os_win=1&version_code=11.3.0&app_version=11.3.0.14362&channel=official&region=CN&app_language=zh-Hans&platform_version=10.0";
const UA: &str = "build_douyin_acount music-collector";
/// 每分类默认/上限采集数量。
pub const PER_CATEGORY_DEFAULT: u32 = 50;
pub const PER_CATEGORY_MAX: u32 = 200;
/// 每首歌下载间隔（礼貌 + 防风控）。
const DOWNLOAD_INTERVAL: Duration = Duration::from_millis(80);

static ACTIVE_COLLECT: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

// ---------- 进度 ----------

/// 采集进度（序列化写 progress 文件，前端轮询展示）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectProgress {
    /// fetching（拉分类/歌曲列表）| downloading | done | stopped | error（interrupted 读出时现判）
    pub stage: String,
    pub category: String,
    /// 当前分类序号（1 基）与分类总数。
    pub category_index: u32,
    pub category_count: u32,
    pub downloaded: u32,
    pub skipped: u32,
    pub failed: u32,
    /// 计划新下载总数（全部分类拉完、扣除已存在后确定）。
    pub planned_total: u32,
    pub bytes_downloaded: u64,
    pub current_song: String,
    pub message: String,
    pub updated_at: u64,
}

impl CollectProgress {
    fn new() -> Self {
        Self {
            stage: "fetching".to_string(),
            category: String::new(),
            category_index: 0,
            category_count: 0,
            downloaded: 0,
            skipped: 0,
            failed: 0,
            planned_total: 0,
            bytes_downloaded: 0,
            current_song: String::new(),
            message: "正在获取音乐分类…".to_string(),
            updated_at: 0,
        }
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn progress_path(store: &MediaStore) -> PathBuf {
    store.music_dir().join(".collect_progress.json")
}

/// 先写 tmp 再 rename，前端不会读到半份 JSON（同 whisper 下载）。
fn write_progress(path: &Path, p: &mut CollectProgress) {
    p.updated_at = unix_now();
    if let Ok(json) = serde_json::to_vec_pretty(p) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// 读取采集进度；非终态且 progress 文件 10s 未更新 → 采集线程已死（应用重启等），判 interrupted。
pub fn collect_status(store: &MediaStore) -> Option<CollectProgress> {
    let pp = progress_path(store);
    let mut p: CollectProgress =
        serde_json::from_str(&std::fs::read_to_string(&pp).ok()?).ok()?;
    if matches!(p.stage.as_str(), "fetching" | "downloading") {
        let stale = std::fs::metadata(&pp)
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().map(|e| e > Duration::from_secs(10)).unwrap_or(false))
            .unwrap_or(false);
        if stale {
            p.stage = "interrupted".to_string();
            p.message = "采集中断（应用可能重启过），重新开始可从断点续采".to_string();
        }
    }
    Some(p)
}

// ---------- 启动 / 取消 ----------

/// 开始后台采集；已有采集中进行时报错。per_category  clamp 到 1..=PER_CATEGORY_MAX。
pub fn start_collect(
    db: Arc<Database>,
    store: MediaStore,
    per_category: u32,
) -> Result<(), DomainError> {
    let per_category = per_category.clamp(1, PER_CATEGORY_MAX);
    {
        let guard = ACTIVE_COLLECT.lock().expect("collect lock poisoned");
        if guard.is_some() {
            return Err(DomainError::InvalidData(
                "音乐库采集正在进行中，请先完成或停止".to_string(),
            ));
        }
    }
    std::fs::create_dir_all(store.music_dir())?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = ACTIVE_COLLECT.lock().expect("collect lock poisoned");
        *guard = Some(cancel.clone());
    }
    let pp = progress_path(&store);
    // 启动线程前先覆盖写初始 fetching 进度：旧文件的 done/stopped 残留会让前端
    // 首次轮询误判「已结束」停轮询（用户随后再点开始撞「进行中」报错）。
    // 同步写入发生在 start 返回前，前端 start 成功后第一次轮询必读到新状态。
    let mut initial = CollectProgress::new();
    initial.message = format!("正在启动采集（每分类 {per_category} 首）…");
    write_progress(&pp, &mut initial);
    std::thread::spawn(move || {
        if let Err(e) = run_collect(&db, &store, per_category, API_BASE, &pp, &cancel) {
            tracing::warn!("music collect ended: {e}");
        }
        let mut guard = ACTIVE_COLLECT.lock().expect("collect lock poisoned");
        *guard = None;
    });
    Ok(())
}

/// 停止进行中的采集（已下载的保留，重跑幂等续采）。
pub fn cancel_collect() -> Result<(), DomainError> {
    let guard = ACTIVE_COLLECT.lock().expect("collect lock poisoned");
    match guard.as_ref() {
        Some(flag) => {
            flag.store(true, Ordering::Relaxed);
            Ok(())
        }
        _ => Err(DomainError::InvalidData(
            "当前没有进行中的音乐库采集".to_string(),
        )),
    }
}

// ---------- API ----------

#[derive(Debug, Clone)]
pub struct Collection {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SongEntry {
    pub id: u64,
    pub title: String,
    pub author: String,
    pub cover_url: Option<String>,
    pub preview_url: String,
}

#[derive(Deserialize)]
struct ApiResp<T> {
    /// 实测是字符串 "0"；用 Value 收兼容数字形态。
    ret: serde_json::Value,
    errmsg: Option<String>,
    data: Option<T>,
}

#[derive(Deserialize)]
struct CollectionsData {
    collections: Vec<CollectionRaw>,
}

#[derive(Deserialize)]
struct CollectionRaw {
    id: u64,
    name: String,
}

#[derive(Deserialize)]
struct SongsData {
    songs: Vec<SongRaw>,
    has_more: Option<bool>,
    next_offset: Option<u64>,
}

#[derive(Deserialize)]
struct SongRaw {
    id: u64,
    title: Option<String>,
    author: Option<String>,
    cover_url: Option<CoverUrlRaw>,
    preview_url: Option<String>,
}

#[derive(Deserialize)]
struct CoverUrlRaw {
    hd: Option<String>,
    large: Option<String>,
    medium: Option<String>,
    thumb: Option<String>,
}

impl CoverUrlRaw {
    fn best(self) -> Option<String> {
        self.hd.or(self.large).or(self.medium).or(self.thumb)
    }
}

fn http_client() -> Result<reqwest::Client, DomainError> {
    reqwest::Client::builder()
        .user_agent(UA)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| DomainError::InvalidData(format!("采集初始化网络客户端失败: {e}")))
}

async fn post_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    base: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<T, DomainError> {
    let url = format!("{base}{path}?{COMMON_QUERY}");
    let resp: ApiResp<T> = client
        .post(&url)
        .json(body)
        .send()
        .await
        .map_err(|e| DomainError::InvalidData(format!("{path} 请求失败: {e}")))?
        .error_for_status()
        .map_err(|e| DomainError::InvalidData(format!("{path} HTTP 错误: {e}")))?
        .json()
        .await
        .map_err(|e| DomainError::InvalidData(format!("{path} 响应解析失败: {e}")))?;
    let ok = resp.ret.as_str() == Some("0") || resp.ret.as_i64() == Some(0);
    if !ok {
        return Err(DomainError::InvalidData(format!(
            "{path} 返回错误: {}（ret={}）",
            resp.errmsg.unwrap_or_default(),
            resp.ret
        )));
    }
    resp.data
        .ok_or_else(|| DomainError::InvalidData(format!("{path} 响应缺少 data")))
}

async fn fetch_collections(
    client: &reqwest::Client,
    base: &str,
) -> Result<Vec<Collection>, DomainError> {
    let data: CollectionsData = post_json(
        client,
        base,
        "/lv/v1/get_collections",
        &serde_json::json!({"scene": 0, "query_type": 0, "cursor": 0, "count": 100}),
    )
    .await?;
    Ok(data
        .collections
        .into_iter()
        .map(|c| Collection { id: c.id, name: c.name })
        .collect())
}

/// 拉一个分类的歌曲，翻页补足 want 首（song_id 去重；服务端 count=50 实测只回 48）。
async fn fetch_songs(
    client: &reqwest::Client,
    base: &str,
    collection_id: u64,
    want: u32,
) -> Result<Vec<SongEntry>, DomainError> {
    let mut out: Vec<SongEntry> = Vec::new();
    let mut seen: HashSet<u64> = HashSet::new();
    let mut cursor: u64 = 0;
    loop {
        let data: SongsData = post_json(
            client,
            base,
            "/lv/v1/get_collection_songs",
            &serde_json::json!({"id": collection_id, "cursor": cursor, "count": 50}),
        )
        .await?;
        for s in data.songs {
            if !seen.insert(s.id) {
                continue;
            }
            let Some(preview_url) = s.preview_url.filter(|u| !u.is_empty()) else {
                continue;
            };
            out.push(SongEntry {
                id: s.id,
                title: s.title.unwrap_or_default(),
                author: s.author.unwrap_or_default(),
                cover_url: s.cover_url.and_then(CoverUrlRaw::best),
                preview_url,
            });
        }
        if out.len() as u32 >= want || !data.has_more.unwrap_or(false) {
            break;
        }
        // 翻页；next_offset 缺失或不前进时防死循环。
        let next = data.next_offset.unwrap_or(cursor + 50);
        if next <= cursor {
            break;
        }
        cursor = next;
    }
    out.truncate(want as usize);
    Ok(out)
}

// ---------- music_categories 表 ----------

/// song_id 已落库的目标文件名（任一分类的映射都行，文件唯一）。
fn song_file_name(db: &Database, song_id: u64) -> Result<Option<String>, DomainError> {
    let conn = db.conn();
    let mut stmt =
        conn.prepare("SELECT file_name FROM music_categories WHERE song_id = ?1 LIMIT 1")?;
    let name = stmt
        .query_row(rusqlite::params![song_id.to_string()], |r| r.get(0))
        .optional()?;
    Ok(name)
}

fn upsert_mapping(
    db: &Database,
    song_id: u64,
    file_name: &str,
    category: &str,
    ordinal: u32,
) -> Result<(), DomainError> {
    db.conn().execute(
        "INSERT OR IGNORE INTO music_categories (song_id, file_name, category, ordinal)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![song_id.to_string(), file_name, category, ordinal],
    )?;
    Ok(())
}

/// 文件被用户删除后清掉该 song 的旧映射（重跑采集会重新下载）。
fn delete_song_rows(db: &Database, song_id: u64) -> Result<(), DomainError> {
    db.conn().execute(
        "DELETE FROM music_categories WHERE song_id = ?1",
        rusqlite::params![song_id.to_string()],
    )?;
    Ok(())
}

/// file_name → 分类列表（按 ordinal 排序），添加音乐弹窗分类栏目数据源。
pub fn categories_map(db: &Database) -> Result<HashMap<String, Vec<String>>, DomainError> {
    let conn = db.conn();
    let mut stmt = conn.prepare(
        "SELECT file_name, category, ordinal FROM music_categories ORDER BY ordinal ASC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u32>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    drop(conn);
    let mut map: HashMap<String, Vec<(u32, String)>> = HashMap::new();
    for (file_name, category, ordinal) in rows {
        map.entry(file_name).or_default().push((ordinal, category));
    }
    Ok(map
        .into_iter()
        .map(|(k, mut v)| {
            v.sort_by_key(|(o, _)| *o);
            v.dedup_by(|a, b| a.1 == b.1);
            (k, v.into_iter().map(|(_, c)| c).collect())
        })
        .collect())
}

// ---------- 采集主流程 ----------

/// 同步入口（下载线程/测试直接调用）：拉分类 → 拉歌曲 → 逐首下载落盘。
pub fn run_collect(
    db: &Arc<Database>,
    store: &MediaStore,
    per_category: u32,
    api_base: &str,
    pp: &Path,
    cancel: &AtomicBool,
) -> Result<(), DomainError> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| DomainError::InvalidData(format!("tokio runtime: {e}")))?;
    rt.block_on(collect_async(db, store, per_category, api_base, pp, cancel))
}

async fn collect_async(
    db: &Arc<Database>,
    store: &MediaStore,
    per_category: u32,
    api_base: &str,
    pp: &Path,
    cancel: &AtomicBool,
) -> Result<(), DomainError> {
    let client = http_client()?;
    let music_dir = store.music_dir();
    let mut p = CollectProgress::new();
    write_progress(pp, &mut p);

    // 1) 分类列表（失败则整体失败——没有分类什么都干不了）。
    let collections = match fetch_collections(&client, api_base).await {
        Ok(c) if !c.is_empty() => c,
        Ok(_) => {
            p.stage = "error".to_string();
            p.message = "音乐分类列表为空（剪映接口可能已变更）".to_string();
            write_progress(pp, &mut p);
            return Err(DomainError::InvalidData(p.message));
        }
        Err(e) => {
            p.stage = "error".to_string();
            p.message = format!("获取音乐分类失败: {e}");
            write_progress(pp, &mut p);
            return Err(e);
        }
    };
    p.category_count = collections.len() as u32;
    write_progress(pp, &mut p);

    // 2) 逐分类拉歌曲（单分类失败跳过不中断）；计划 = (分类名, ordinal, 歌曲) 列表。
    // 同一首歌出现在多个分类时**每分类都进计划**：首个分类下载落库后，后续分类走
    // 「已有映射」分支补该分类的映射行（不会重复下载，见下载循环）。
    let mut planned: Vec<(String, u32, SongEntry)> = Vec::new();
    for (idx, c) in collections.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            p.stage = "stopped".to_string();
            p.message = "已停止（已下载的保留，重新开始可续采）".to_string();
            write_progress(pp, &mut p);
            return Ok(());
        }
        p.category = c.name.clone();
        p.category_index = idx as u32 + 1;
        p.message = format!("正在获取「{}」歌曲列表…", c.name);
        write_progress(pp, &mut p);
        match fetch_songs(&client, api_base, c.id, per_category).await {
            Ok(songs) => {
                for s in songs {
                    planned.push((c.name.clone(), idx as u32, s));
                }
            }
            Err(e) => {
                tracing::warn!("拉取分类「{}」歌曲失败（跳过）: {e}", c.name);
                p.message = format!("分类「{}」获取失败，已跳过", c.name);
                write_progress(pp, &mut p);
            }
        }
    }

    // 3) 计划新下载数：无有效映射（未落库或文件已被删）的歌曲数（按 song_id 去重——
    // 跨分类重复的歌只下载一次，进度分母不能虚高）。
    let mut need = 0u32;
    let mut need_ids: HashSet<u64> = HashSet::new();
    for (_, _, s) in &planned {
        let existing = song_file_name(db, s.id)?;
        let valid = existing
            .as_ref()
            .map(|n| music_dir.join(n).is_file())
            .unwrap_or(false);
        if !valid && need_ids.insert(s.id) {
            need += 1;
        }
    }
    p.stage = "downloading".to_string();
    p.planned_total = need;
    p.message = format!("共 {} 首待下载", need);
    write_progress(pp, &mut p);

    // 4) 逐首下载。
    let mut md5_cache: HashMap<PathBuf, (u64, String)> = HashMap::new();
    for (category, ordinal, song) in &planned {
        if cancel.load(Ordering::Relaxed) {
            p.stage = "stopped".to_string();
            p.message = format!(
                "已停止：新下载 {} 首、跳过 {} 首（重跑可续采）",
                p.downloaded, p.skipped
            );
            write_progress(pp, &mut p);
            return Ok(());
        }
        p.current_song = format!("{} - {}", song.title, song.author);
        write_progress(pp, &mut p);

        // 已有有效映射：补缺失分类行 + 补封面，不重复下载。
        if let Some(existing) = song_file_name(db, song.id)? {
            if music_dir.join(&existing).is_file() {
                upsert_mapping(db, song.id, &existing, category, *ordinal)?;
                if let Some(url) = &song.cover_url {
                    music_covers::upsert_exact_url(db, &existing, url)?;
                }
                p.skipped += 1;
                continue;
            }
            // 文件被用户删除：清旧映射重新下载。
            delete_song_rows(db, song.id)?;
        }

        match download_one(&client, &music_dir, song, &mut md5_cache).await {
            Ok((file_name, size)) => {
                upsert_mapping(db, song.id, &file_name, category, *ordinal)?;
                if let Some(url) = &song.cover_url {
                    music_covers::upsert_exact_url(db, &file_name, url)?;
                }
                p.downloaded += 1;
                p.bytes_downloaded += size;
            }
            Err(e) => {
                tracing::warn!("下载失败（跳过）{}: {e}", p.current_song);
                p.failed += 1;
            }
        }
        write_progress(pp, &mut p);
        tokio::time::sleep(DOWNLOAD_INTERVAL).await;
    }

    p.stage = "done".to_string();
    p.current_song = String::new();
    p.message = format!(
        "采集完成：新下载 {} 首、已存在 {} 首{}",
        p.downloaded,
        p.skipped,
        if p.failed > 0 {
            format!("、失败 {} 首", p.failed)
        } else {
            String::new()
        }
    );
    write_progress(pp, &mut p);
    Ok(())
}

/// 下载一首歌到 music_dir：整请求读入（单首 ~2MB）→ `.part` → 按真实格式+命名规则落位。
/// 与剪映缓存同步共用 sanitize/pick_dest 规则：同内容已存在时复用该文件名（不重复落盘）。
/// 返回 (目标文件名, 网络下载字节数)。
async fn download_one(
    client: &reqwest::Client,
    music_dir: &Path,
    song: &SongEntry,
    md5_cache: &mut HashMap<PathBuf, (u64, String)>,
) -> Result<(String, u64), DomainError> {
    let mut last_err = String::new();
    // 失败重试 2 次（共 3 次）。
    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(500 * attempt as u64)).await;
        }
        match try_download(client, music_dir, song, md5_cache).await {
            Ok(name) => return Ok(name),
            Err(e) => last_err = e.to_string(),
        }
    }
    Err(DomainError::InvalidData(last_err))
}

async fn try_download(
    client: &reqwest::Client,
    music_dir: &Path,
    song: &SongEntry,
    md5_cache: &mut HashMap<PathBuf, (u64, String)>,
) -> Result<(String, u64), DomainError> {
    let bytes = client
        .get(&song.preview_url)
        .send()
        .await
        .map_err(|e| DomainError::InvalidData(format!("下载请求失败: {e}")))?
        .error_for_status()
        .map_err(|e| DomainError::InvalidData(format!("下载 HTTP 错误: {e}")))?
        .bytes()
        .await
        .map_err(|e| DomainError::InvalidData(format!("下载读取失败: {e}")))?;
    // 防错误页/空响应：音乐文件不会小于 16KB。
    if bytes.len() < 16 * 1024 {
        return Err(DomainError::InvalidData(format!(
            "下载内容异常（{} 字节）",
            bytes.len()
        )));
    }
    let part = music_dir.join(format!(".{}.part", song.id));
    std::fs::write(&part, &bytes)?;
    let size = bytes.len() as u64;
    let result = place_downloaded(music_dir, song, &part, md5_cache);
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result.map(|name| (name, size))
}

/// `.part` → 正式文件名（真实格式探测 + 重名规则），返回目标文件名。
fn place_downloaded(
    music_dir: &Path,
    song: &SongEntry,
    part: &Path,
    md5_cache: &mut HashMap<PathBuf, (u64, String)>,
) -> Result<String, DomainError> {
    let ext = music_sync::detect_ext(part)?;
    let base = if song.author.is_empty() {
        music_sync::sanitize(&song.title)
    } else {
        music_sync::sanitize(&format!("{} - {}", song.title, song.author))
    };
    let size = part.metadata()?.len();
    let md5 = music_sync::md5_file(part)?;
    match music_sync::pick_dest(music_dir, &base, &ext, size, &md5, md5_cache)? {
        music_sync::DestAction::Skip(dest) => {
            // 同内容已存在（剪映同步过/上一分类下过）：复用其文件名，不留重复文件。
            let _ = std::fs::remove_file(part);
            Ok(dest
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default())
        }
        music_sync::DestAction::Copy(dest) => {
            std::fs::rename(part, &dest)?;
            md5_cache.insert(dest.clone(), (size, md5));
            Ok(dest
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default())
        }
    }
}
