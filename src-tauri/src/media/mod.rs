use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::db::Database;
use crate::db::repos::assets::AssetsRepo;
use crate::domain::asset::{AssetStatus, NewAsset, VideoAsset};
use crate::domain::error::DomainError;
use crate::util::run_tool;

/// 本地媒体文件存储布局。
#[derive(Debug, Clone)]
pub struct MediaStore {
    root: PathBuf,
}

impl MediaStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn videos_dir(&self) -> PathBuf {
        self.root.join("videos")
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.root.join("covers")
    }

    pub fn frames_dir(&self) -> PathBuf {
        self.root.join("frames")
    }

    pub fn audio_dir(&self) -> PathBuf {
        self.root.join("audio")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn db_dir(&self) -> PathBuf {
        self.root.join("db")
    }

    pub fn tools_dir(&self) -> PathBuf {
        self.root.join("tools")
    }

    /// 剪映缓存音乐同步落点（`music/`，备份 zip 自动包含——用户内容资产）。
    pub fn music_dir(&self) -> PathBuf {
        self.root.join("music")
    }

    /// 音乐专辑封面落点（`music/covers/`，与音乐的映射见 music_covers 表）。
    pub fn music_covers_dir(&self) -> PathBuf {
        self.root.join("music").join("covers")
    }

    /// Whisper 内置模型根目录（`models/whisper/<模型名>/`，由应用自行下载管理）。
    pub fn models_dir(&self) -> PathBuf {
        self.root.join("models").join("whisper")
    }

    /// 创建全部子目录（幂等）。
    pub fn ensure_dirs(&self) -> Result<(), DomainError> {
        for dir in [
            self.root.as_path(),
            self.videos_dir().as_path(),
            self.covers_dir().as_path(),
            self.frames_dir().as_path(),
            self.audio_dir().as_path(),
            self.logs_dir().as_path(),
            self.db_dir().as_path(),
            self.tools_dir().as_path(),
            self.models_dir().as_path(),
            self.music_dir().as_path(),
            self.music_covers_dir().as_path(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// 导入本地视频时的元数据。
#[derive(Debug, Clone)]
pub struct ImportOptions {
    pub source_platform: String,
    pub title: Option<String>,
    pub source_url: Option<String>,
    pub author: Option<String>,
    pub dedup_key: Option<String>,
}

/// 视频管理服务：导入、删除、收藏、状态流转，并负责磁盘文件生命周期。
pub struct VideoManager<'a> {
    db: &'a Database,
    store: MediaStore,
}

impl<'a> VideoManager<'a> {
    pub fn new(db: &'a Database, store: MediaStore) -> Self {
        Self { db, store }
    }

    pub fn get_asset(&self, id: i64) -> Result<Option<VideoAsset>, DomainError> {
        AssetsRepo::new(self.db).get(id)
    }

    pub fn update_status(&self, id: i64, to: AssetStatus) -> Result<(), DomainError> {
        AssetsRepo::new(self.db).update_status(id, to)
    }

    /// 导入本地视频：探测时长 -> 复制到媒体库 -> 生成封面 -> 入库。
    pub fn import_file(&self, src: &Path, opts: ImportOptions) -> Result<VideoAsset, DomainError> {
        if !src.is_file() {
            return Err(DomainError::InvalidData(format!(
                "source file not found: {}",
                src.display()
            )));
        }
        let duration_ms = probe_duration_ms(src)?;
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp4")
            .to_string();
        let id = Uuid::new_v4();
        let video_path = self.store.videos_dir().join(format!("{id}.{ext}"));
        let cover_path = self.store.covers_dir().join(format!("{id}.jpg"));
        std::fs::copy(src, &video_path)?;

        // 封面生成失败不阻塞导入（封面可后续重试生成）。
        if let Err(e) = generate_cover(&video_path, &cover_path) {
            tracing::warn!("cover generation failed for {id}: {e}");
        }

        let asset_id = AssetsRepo::new(self.db).insert(NewAsset {
            dedup_key: opts.dedup_key,
            file_path: video_path.to_string_lossy().into_owned(),
            cover_path: Some(cover_path.to_string_lossy().into_owned()),
            source_platform: opts.source_platform,
            source_url: opts.source_url,
            author: opts.author,
            title: opts.title,
            description: None,
            published_at: None,
            tags: Vec::new(),
            likes: 0,
            comments: 0,
            plays: 0,
            duration_ms,
        })?;
        Ok(self
            .get_asset(asset_id)?
            .expect("just inserted asset must exist"))
    }

    /// 删除资产：先清理磁盘文件（视频/封面/抽帧/音频/音乐合成原件），再删除数据库记录。
    pub fn delete_asset(&self, id: i64) -> Result<(), DomainError> {
        let asset = self
            .get_asset(id)?
            .ok_or(DomainError::AssetNotFound(id))?;
        // 只允许删除媒体库内的文件，避免误删库外路径。
        for p in [asset.file_path.as_str(), asset.cover_path.as_deref().unwrap_or("")] {
            if !p.is_empty() {
                remove_if_in_store(&self.store, Path::new(p));
            }
        }
        // 音乐合成前的原始视频（file_path 已指向合成产物，原件靠 music_info 追踪）。
        if let Some(info) = &asset.music_info {
            remove_if_in_store(&self.store, Path::new(&info.origin_file_path));
        }
        let frames = self.store.frames_dir().join(id.to_string());
        let _ = std::fs::remove_dir_all(&frames);
        let audio = self.store.audio_dir().join(format!("{id}.wav"));
        let _ = std::fs::remove_file(&audio);
        AssetsRepo::new(self.db).delete(id)
    }
}

/// 仅删除位于媒体库根目录内的文件（忽略 NotFound）。
fn remove_if_in_store(store: &MediaStore, path: &Path) {
    let root = store.root();
    if path.starts_with(root) {
        match std::fs::remove_file(path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!("failed to remove {}: {e}", path.display()),
        }
    }
}

/// 用 ffprobe 探测视频时长（毫秒）。供媒体服务与分析管线复用。
pub(crate) fn probe_duration_ms(path: &Path) -> Result<i64, DomainError> {
    let out = run_tool(
        "ffprobe",
        &[
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
            path.to_string_lossy().as_ref(),
        ],
    )?;
    let value: serde_json::Value = serde_json::from_slice(&out)?;
    let duration_s = value["format"]["duration"]
        .as_str()
        .ok_or_else(|| DomainError::InvalidData("ffprobe: duration missing".to_string()))?;
    let secs: f64 = duration_s
        .parse()
        .map_err(|_| DomainError::InvalidData(format!("bad duration: {duration_s}")))?;
    Ok((secs * 1000.0).round() as i64)
}

/// 提取视频第一帧（0.1s 处）生成封面。
fn generate_cover(video: &Path, cover: &Path) -> Result<(), DomainError> {
    run_tool(
        "ffmpeg",
        &[
            "-y",
            "-ss",
            "0.1",
            "-i",
            video.to_string_lossy().as_ref(),
            "-vframes",
            "1",
            "-vf",
            "scale=480:-1",
            cover.to_string_lossy().as_ref(),
        ],
    )?;
    Ok(())
}
