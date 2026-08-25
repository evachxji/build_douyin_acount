//! Whisper 模型内置化管理：注册表、安装检测、带路由回退的断点续传下载器。
//!
//! 模型统一存放在 `storage/models/whisper/<名称>/`，由应用自行下载管理（内置基座，
//! 不依赖用户预装的 HF 全局缓存）。faster-whisper 支持直接加载本地目录，
//! `whisper_transcribe.py` 无需改动——`state.rs` 把已安装模型的目录绝对路径传给它即可。
//!
//! 下载器在专用线程运行（不走 agent runtime：设置语义的即时操作，且分钟级下载
//! 会卡住 `Agent::run` 的 2s 轮询线程）。进度写入 `.progress_<名称>.json`（tmp+rename），
//! 前端经 `list_whisper_models` 轮询；非终态且文件 10s 未更新视为中断（应用重启），
//! `.part` 断点保留，重新开始后续传。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use serde::Serialize;

use crate::domain::error::DomainError;
use crate::media::MediaStore;

/// 官方源与国内镜像（hf-mirror.com 与 huggingface.co 的 resolve 路径完全一致）。
pub const OFFICIAL_HOST: &str = "https://huggingface.co";
pub const MIRROR_HOST: &str = "https://hf-mirror.com";

/// 逐 chunk 读取超时：超过 30s 无数据判定本次尝试失败（也是取消标记的最长响应时间）。
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// 慢速守卫：自动模式下官方源均速低于该值且持续 20s，切换镜像续传。
const SLOW_BPS: u64 = 50 * 1024;
const SLOW_AFTER: Duration = Duration::from_secs(20);
/// 单个文件最多尝试次数（自动换路由不消耗次数）。
const MAX_ATTEMPTS: usize = 3;

/// 单个 Whisper 模型规格的静态描述（体积/显存标注供设置页下拉框展示）。
pub struct WhisperModelSpec {
    pub name: &'static str,
    pub size_mb: u32,
    pub vram_hint: &'static str,
    pub files: &'static [&'static str],
}

/// 可选模型清单（Systran/faster-whisper-* 仓库，文件清单已与 HF API 核实）。
pub const MODELS: &[WhisperModelSpec] = &[
    WhisperModelSpec {
        name: "tiny",
        size_mb: 75,
        vram_hint: "显存约 1GB，纯 CPU 可跑",
        files: &["config.json", "model.bin", "tokenizer.json", "vocabulary.txt"],
    },
    WhisperModelSpec {
        name: "base",
        size_mb: 145,
        vram_hint: "显存约 1GB，纯 CPU 可跑",
        files: &["config.json", "model.bin", "tokenizer.json", "vocabulary.txt"],
    },
    WhisperModelSpec {
        name: "small",
        size_mb: 466,
        vram_hint: "显存约 2GB",
        files: &["config.json", "model.bin", "tokenizer.json", "vocabulary.txt"],
    },
    WhisperModelSpec {
        name: "medium",
        size_mb: 1500,
        vram_hint: "显存约 5GB",
        files: &["config.json", "model.bin", "tokenizer.json", "vocabulary.txt"],
    },
    WhisperModelSpec {
        name: "large-v3",
        size_mb: 3100,
        vram_hint: "显存约 10GB",
        files: &[
            "config.json",
            "model.bin",
            "preprocessor_config.json",
            "tokenizer.json",
            "vocabulary.json",
        ],
    },
];

pub fn find_spec(name: &str) -> Option<&'static WhisperModelSpec> {
    MODELS.iter().find(|m| m.name == name)
}

/// 模型安装目录（`storage/models/whisper/<名称>/`）。
pub fn whisper_model_dir(store: &MediaStore, name: &str) -> PathBuf {
    store.models_dir().join(name)
}

fn progress_path_for(store: &MediaStore, name: &str) -> PathBuf {
    store.models_dir().join(format!(".progress_{name}.json"))
}

/// 安装判据：清单文件齐全且 model.bin > 1MB（完整文件由 .part rename 而来，天然原子）。
pub fn is_installed(spec: &WhisperModelSpec, dir: &Path) -> bool {
    spec.files.iter().all(|f| dir.join(f).is_file())
        && std::fs::metadata(dir.join("model.bin"))
            .map(|m| m.len() > 1_000_000)
            .unwrap_or(false)
}

/// 把设置里的模型名解析为 faster-whisper 可加载的参数：
/// 已安装的已知名 → 本地目录绝对路径；未安装/非已知名（旧自定义值）→ 原样透传，
/// 由 faster-whisper 回退自身缓存解析（向后兼容，UI 层会引导用户下载）。
pub fn resolve_model_arg(store: &MediaStore, configured: &str) -> String {
    if let Some(spec) = find_spec(configured) {
        let dir = whisper_model_dir(store, configured);
        if is_installed(spec, &dir) {
            return dir.to_string_lossy().into_owned();
        }
        tracing::warn!(
            "whisper model {configured} 未安装到本地目录，回退 faster-whisper 默认解析（请到设置页下载模型）"
        );
    }
    configured.to_string()
}

/// 下载路由偏好（auto = 官方优先、失败或过慢自动切镜像兜底）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePref {
    Auto,
    Official,
    Mirror,
}

impl RoutePref {
    pub fn parse(s: &str) -> Result<Self, DomainError> {
        match s {
            "auto" => Ok(Self::Auto),
            "official" => Ok(Self::Official),
            "mirror" => Ok(Self::Mirror),
            other => Err(DomainError::InvalidData(format!(
                "unknown download route: {other}"
            ))),
        }
    }
}

/// 实际使用的路由（auto 模式下会随回退切换）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Route {
    Official,
    Mirror,
}

impl Route {
    fn label(self) -> &'static str {
        match self {
            Route::Official => "official",
            Route::Mirror => "mirror",
        }
    }

    fn cn_label(self) -> &'static str {
        match self {
            Route::Official => "官方源",
            Route::Mirror => "国内镜像",
        }
    }
}

/// 下载源地址（测试注入本地 mock server；生产用默认值）。
#[derive(Debug, Clone)]
pub struct Hosts {
    pub official: String,
    pub mirror: String,
}

impl Default for Hosts {
    fn default() -> Self {
        Self {
            official: OFFICIAL_HOST.to_string(),
            mirror: MIRROR_HOST.to_string(),
        }
    }
}

impl Hosts {
    fn url(&self, route: Route, spec: &WhisperModelSpec, file: &str) -> String {
        let host = match route {
            Route::Official => &self.official,
            Route::Mirror => &self.mirror,
        };
        format!("{host}/Systran/faster-whisper-{}/resolve/main/{file}", spec.name)
    }
}

/// 下载进度（序列化写入 progress 文件，前端轮询展示）。
#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub model: String,
    /// connecting | downloading | verifying | done | error | cancelled
    pub stage: String,
    pub route: String,
    pub current_file: String,
    pub file_index: usize,
    pub file_count: usize,
    pub file_downloaded: u64,
    pub file_total: u64,
    pub total_downloaded: u64,
    pub total_bytes: u64,
    pub speed_bps: u64,
    pub url: String,
    pub dest: String,
    pub message: String,
    pub updated_at: u64,
}

impl DownloadProgress {
    fn new(spec: &WhisperModelSpec, dest: &Path) -> Self {
        Self {
            model: spec.name.to_string(),
            stage: "connecting".to_string(),
            route: Route::Official.label().to_string(),
            current_file: String::new(),
            file_index: 0,
            file_count: spec.files.len(),
            file_downloaded: 0,
            file_total: 0,
            total_downloaded: 0,
            total_bytes: spec.size_mb as u64 * 1_000_000,
            speed_bps: 0,
            url: String::new(),
            dest: dest.to_string_lossy().into_owned(),
            message: String::new(),
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

/// 先写 tmp 再 rename，前端不会读到半份 JSON。
fn write_progress(path: &Path, p: &mut DownloadProgress) {
    p.updated_at = unix_now();
    if let Ok(json) = serde_json::to_vec_pretty(p) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// 下载中断原因（Slow 仅自动模式官方源触发，外层据此换镜像续传）。
enum Abort {
    Cancelled,
    Slow,
    Failed(String),
}

/// 供前端展示的单个模型状态。
#[derive(Debug, Serialize)]
pub struct WhisperModelStatus {
    pub name: String,
    pub size_mb: u32,
    pub vram_hint: String,
    pub file_count: usize,
    pub installed: bool,
    pub dir: String,
    pub download: Option<serde_json::Value>,
}

pub fn list_models(store: &MediaStore) -> Vec<WhisperModelStatus> {
    MODELS
        .iter()
        .map(|spec| {
            let dir = whisper_model_dir(store, spec.name);
            let installed = is_installed(spec, &dir);
            let pp = progress_path_for(store, spec.name);
            let mut download = std::fs::read_to_string(&pp)
                .ok()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
            // 心跳判活：非终态但 progress 文件 10s 未更新 → 下载线程已死（应用重启等），
            // 标记中断，前端提示可从断点续传。
            if let Some(v) = download.as_mut() {
                let stage = v.get("stage").and_then(|s| s.as_str()).unwrap_or("");
                if matches!(stage, "connecting" | "downloading" | "verifying") {
                    let stale = std::fs::metadata(&pp)
                        .and_then(|m| m.modified())
                        .map(|t| t.elapsed().map(|e| e > Duration::from_secs(10)).unwrap_or(false))
                        .unwrap_or(false);
                    if stale {
                        if let Some(o) = v.as_object_mut() {
                            o.insert(
                                "stage".to_string(),
                                serde_json::Value::String("interrupted".to_string()),
                            );
                            o.insert(
                                "message".to_string(),
                                serde_json::Value::String(
                                    "下载已中断（应用可能重启过），重新下载可从断点续传".to_string(),
                                ),
                            );
                        }
                    }
                }
            }
            WhisperModelStatus {
                name: spec.name.to_string(),
                size_mb: spec.size_mb,
                vram_hint: spec.vram_hint.to_string(),
                file_count: spec.files.len(),
                installed,
                dir: dir.to_string_lossy().into_owned(),
                download,
            }
        })
        .collect()
}

struct ActiveDownload {
    model: String,
    cancel: Arc<AtomicBool>,
}

/// 全局同时只允许一个模型下载（模型文件大，并行下载互相拖慢且没有实际诉求）。
static ACTIVE_DOWNLOAD: Mutex<Option<ActiveDownload>> = Mutex::new(None);

/// 开始后台下载指定模型；已安装或已有下载进行中时报错。
pub fn start_download(store: &MediaStore, name: &str, route: &str) -> Result<(), DomainError> {
    let spec = find_spec(name)
        .ok_or_else(|| DomainError::InvalidData(format!("unknown whisper model: {name}")))?;
    let pref = RoutePref::parse(route)?;
    {
        let guard = ACTIVE_DOWNLOAD.lock().expect("download lock poisoned");
        if let Some(active) = guard.as_ref() {
            return Err(DomainError::InvalidData(format!(
                "已有模型下载进行中：{}，请先完成或取消",
                active.model
            )));
        }
    }
    let dir = whisper_model_dir(store, name);
    if is_installed(spec, &dir) {
        return Err(DomainError::InvalidData(format!("模型 {name} 已安装")));
    }
    std::fs::create_dir_all(&dir)?;
    let progress_path = progress_path_for(store, name);
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = ACTIVE_DOWNLOAD.lock().expect("download lock poisoned");
        *guard = Some(ActiveDownload {
            model: name.to_string(),
            cancel: cancel.clone(),
        });
    }
    let hosts = Hosts::default();
    let model_name = name.to_string();
    std::thread::spawn(move || {
        if let Err(e) = download_model(spec, &dir, pref, &hosts, &progress_path, cancel) {
            tracing::warn!("whisper model {model_name} download ended: {e}");
        }
        let mut guard = ACTIVE_DOWNLOAD.lock().expect("download lock poisoned");
        *guard = None;
    });
    Ok(())
}

/// 取消进行中的下载（保留 .part 断点，下次续传）。
pub fn cancel_download(name: &str) -> Result<(), DomainError> {
    let guard = ACTIVE_DOWNLOAD.lock().expect("download lock poisoned");
    match guard.as_ref() {
        Some(active) if active.model == name => {
            active.cancel.store(true, Ordering::Relaxed);
            Ok(())
        }
        _ => Err(DomainError::InvalidData(format!(
            "模型 {name} 当前没有进行中的下载"
        ))),
    }
}

/// 删除已安装的模型目录与进度文件（下载中的模型拒绝删除）。
pub fn delete_model(store: &MediaStore, name: &str) -> Result<(), DomainError> {
    if find_spec(name).is_none() {
        return Err(DomainError::InvalidData(format!(
            "unknown whisper model: {name}"
        )));
    }
    {
        let guard = ACTIVE_DOWNLOAD.lock().expect("download lock poisoned");
        if matches!(guard.as_ref(), Some(a) if a.model == name) {
            return Err(DomainError::InvalidData(
                "模型正在下载，请先取消再删除".to_string(),
            ));
        }
    }
    let dir = whisper_model_dir(store, name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    let _ = std::fs::remove_file(progress_path_for(store, name));
    Ok(())
}

/// 下载完整模型（同步，供下载线程/测试直接调用）。
pub fn download_model(
    spec: &'static WhisperModelSpec,
    dest: &Path,
    pref: RoutePref,
    hosts: &Hosts,
    progress_path: &Path,
    cancel: Arc<AtomicBool>,
) -> Result<(), DomainError> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| DomainError::InvalidData(format!("tokio runtime: {e}")))?;
    match rt.block_on(download_async(spec, dest, pref, hosts, progress_path, &cancel)) {
        Ok(()) => Ok(()),
        Err(Abort::Cancelled) => Err(DomainError::InvalidData("下载已取消".to_string())),
        Err(Abort::Slow) => Err(DomainError::InvalidData(
            "官方源速度过慢且无法切换镜像".to_string(),
        )),
        Err(Abort::Failed(msg)) => Err(DomainError::InvalidData(msg)),
    }
}

async fn download_async(
    spec: &'static WhisperModelSpec,
    dest: &Path,
    pref: RoutePref,
    hosts: &Hosts,
    progress_path: &Path,
    cancel: &AtomicBool,
) -> Result<(), Abort> {
    // 不设整体 timeout：大文件下载时长不可预估，停滞由逐 chunk 30s 超时检测。
    let client = reqwest::Client::builder()
        .user_agent("build_douyin_acount whisper-model-downloader")
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| Abort::Failed(e.to_string()))?;

    let mut p = DownloadProgress::new(spec, dest);
    let mut route = match pref {
        RoutePref::Mirror => Route::Mirror,
        _ => Route::Official,
    };
    p.route = route.label().to_string();
    p.message = "正在获取文件信息…".to_string();
    write_progress(progress_path, &mut p);

    // HEAD 逐文件取大小求和作总进度分母；失败回退注册表约值（不影响下载本身）。
    let mut sizes: Vec<Option<u64>> = Vec::with_capacity(spec.files.len());
    for f in spec.files {
        sizes.push(head_size(&client, &hosts.url(route, spec, f)).await);
    }
    if sizes.iter().all(Option::is_some) {
        p.total_bytes = sizes.iter().map(|s| s.unwrap_or(0)).sum();
    }
    // 已完成的文件计入总进度分母之外（直接加到已下载量）。
    let mut base_done: u64 = 0;
    for f in spec.files {
        let final_path = dest.join(f);
        if final_path.is_file() {
            base_done += std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
        }
    }
    p.total_downloaded = base_done.min(p.total_bytes);

    let result = download_files(
        spec,
        dest,
        pref,
        hosts,
        &client,
        &sizes,
        &mut route,
        &mut p,
        progress_path,
        cancel,
        base_done,
    )
    .await;

    match result {
        Ok(()) => {
            p.stage = "verifying".to_string();
            p.speed_bps = 0;
            p.message = "校验文件完整性…".to_string();
            write_progress(progress_path, &mut p);
            if is_installed(spec, dest) {
                p.stage = "done".to_string();
                p.message = "下载完成，模型已就绪".to_string();
                p.total_downloaded = p.total_bytes;
                write_progress(progress_path, &mut p);
                Ok(())
            } else {
                let msg = "文件校验未通过（文件缺失或过小），请重试".to_string();
                p.stage = "error".to_string();
                p.message = msg.clone();
                write_progress(progress_path, &mut p);
                Err(Abort::Failed(msg))
            }
        }
        Err(Abort::Cancelled) => {
            p.stage = "cancelled".to_string();
            p.speed_bps = 0;
            p.message = "已取消，断点已保留（重新下载可续传）".to_string();
            write_progress(progress_path, &mut p);
            Err(Abort::Cancelled)
        }
        Err(e) => {
            let msg = match e {
                Abort::Failed(m) => m,
                Abort::Slow => "官方源速度过慢".to_string(),
                Abort::Cancelled => unreachable!("cancelled handled above"),
            };
            p.stage = "error".to_string();
            p.speed_bps = 0;
            p.message = msg.clone();
            write_progress(progress_path, &mut p);
            Err(Abort::Failed(msg))
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn download_files(
    spec: &'static WhisperModelSpec,
    dest: &Path,
    pref: RoutePref,
    hosts: &Hosts,
    client: &reqwest::Client,
    sizes: &[Option<u64>],
    route: &mut Route,
    p: &mut DownloadProgress,
    progress_path: &Path,
    cancel: &AtomicBool,
    mut base_done: u64,
) -> Result<(), Abort> {
    for (i, f) in spec.files.iter().enumerate() {
        let final_path = dest.join(f);
        if final_path.is_file() {
            continue;
        }
        p.current_file = (*f).to_string();
        p.file_index = i + 1;
        let mut attempt = 0usize;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Abort::Cancelled);
            }
            attempt += 1;
            p.stage = "downloading".to_string();
            let url = hosts.url(*route, spec, f);
            p.url = url.clone();
            p.message = format!("正在下载 {f}（{}）", route.cn_label());
            write_progress(progress_path, p);
            match download_one(
                client,
                &url,
                dest,
                f,
                sizes.get(i).copied().flatten(),
                p,
                progress_path,
                cancel,
                base_done,
                pref == RoutePref::Auto && *route == Route::Official,
            )
            .await
            {
                Ok(()) => {
                    let part = part_path(dest, f);
                    std::fs::rename(&part, &final_path).map_err(|e| {
                        Abort::Failed(format!("保存文件失败：{e}"))
                    })?;
                    base_done += std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
                    p.total_downloaded = base_done.min(p.total_bytes);
                    break;
                }
                Err(Abort::Cancelled) => return Err(Abort::Cancelled),
                Err(Abort::Slow) => {
                    // 仅 auto+official 会触发：切镜像续传，不消耗尝试次数。
                    *route = Route::Mirror;
                    p.route = route.label().to_string();
                    p.message = "官方源速度过慢，已切换国内镜像续传".to_string();
                    write_progress(progress_path, p);
                    attempt = attempt.saturating_sub(1);
                }
                Err(Abort::Failed(msg)) => {
                    if pref == RoutePref::Auto && *route == Route::Official {
                        *route = Route::Mirror;
                        p.route = route.label().to_string();
                        p.message = format!("官方源失败（{msg}），切换国内镜像重试");
                        write_progress(progress_path, p);
                        attempt = attempt.saturating_sub(1);
                        continue;
                    }
                    if attempt >= MAX_ATTEMPTS {
                        return Err(Abort::Failed(format!(
                            "{f} 下载失败（已重试 {MAX_ATTEMPTS} 次）：{msg}"
                        )));
                    }
                    p.message = format!("{f} 第 {attempt} 次尝试失败（{msg}），1s 后重试…");
                    write_progress(progress_path, p);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn download_one(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    file: &str,
    head_size: Option<u64>,
    p: &mut DownloadProgress,
    progress_path: &Path,
    cancel: &AtomicBool,
    base_done: u64,
    slow_guard: bool,
) -> Result<(), Abort> {
    let part = part_path(dest, file);
    let mut resume_from = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut req = client.get(url);
    if resume_from > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }
    let resp = req.send().await.map_err(|e| Abort::Failed(e.to_string()))?;
    let status = resp.status();
    if status.as_u16() == 416 {
        // 服务端认为区间越界：本地 .part 已等同完整文件，直接采用。
        return Ok(());
    }
    if !status.is_success() {
        return Err(Abort::Failed(format!("HTTP {status}")));
    }
    // 服务端忽略 Range 回了 200：截断重来。
    let append = resume_from > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !append {
        resume_from = 0;
    }
    p.file_total = head_size.unwrap_or_else(|| resp.content_length().unwrap_or(0));
    p.file_downloaded = resume_from;

    let mut out = if append {
        std::fs::OpenOptions::new().create(true).append(true).open(&part)
    } else {
        std::fs::File::create(&part)
    }
    .map_err(|e| Abort::Failed(format!("写入失败：{e}")))?;

    let mut stream = resp.bytes_stream();
    let started = Instant::now();
    let mut attempt_bytes: u64 = 0;
    let mut last_flush = Instant::now();
    let mut mark_at = Instant::now();
    let mut mark_bytes = resume_from;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(Abort::Cancelled);
        }
        let item = match tokio::time::timeout(READ_TIMEOUT, stream.next()).await {
            Ok(Some(Ok(chunk))) => chunk,
            Ok(None) => break,
            Ok(Some(Err(e))) => return Err(Abort::Failed(e.to_string())),
            Err(_) => return Err(Abort::Failed("读取超时（30s 无数据）".to_string())),
        };
        out.write_all(&item)
            .map_err(|e| Abort::Failed(format!("写入失败：{e}")))?;
        attempt_bytes += item.len() as u64;
        p.file_downloaded += item.len() as u64;
        p.total_downloaded = (base_done + p.file_downloaded).min(p.total_bytes);
        if last_flush.elapsed() >= Duration::from_millis(500) {
            let dt = mark_at.elapsed().as_secs_f64();
            if dt > 0.0 {
                p.speed_bps = ((p.file_downloaded - mark_bytes) as f64 / dt) as u64;
            }
            mark_at = Instant::now();
            mark_bytes = p.file_downloaded;
            write_progress(progress_path, p);
            last_flush = Instant::now();
        }
        if slow_guard {
            let el = started.elapsed();
            if el > SLOW_AFTER && attempt_bytes / el.as_secs().max(1) < SLOW_BPS {
                return Err(Abort::Slow);
            }
        }
    }
    out.flush()
        .map_err(|e| Abort::Failed(format!("写入失败：{e}")))?;
    // 完整性：已知大小却明显不符 → 判定失败（.part 保留，下次续传）。
    if p.file_total > 0 && p.file_downloaded != p.file_total {
        return Err(Abort::Failed(format!(
            "大小不符 {}/{}",
            p.file_downloaded, p.file_total
        )));
    }
    Ok(())
}

fn part_path(dest: &Path, file: &str) -> PathBuf {
    dest.join(format!("{file}.part"))
}

async fn head_size(client: &reqwest::Client, url: &str) -> Option<u64> {
    let resp = client.head(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    // 直接读 Content-Length 头：HEAD 响应 reqwest 的 content_length() 恒为 0。
    resp.headers()
        .get(reqwest::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}
