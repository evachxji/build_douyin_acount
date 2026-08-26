use serde::{Deserialize, Serialize};

use super::error::DomainError;

/// 视频资产生命周期状态。
///
/// NEW（待处理）-> 采集入库；
/// ANALYZING（分析中）-> AI 打分中；
/// CANDIDATE（候选）-> 质量合格待人工挑选；
/// ELIMINATED（淘汰）-> 质量不佳/不适合，统一淘汰；
/// SELECTED（待发布）-> 用户「通过」后等待发布；
/// PUBLISHED（已发布）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AssetStatus {
    New,
    Analyzing,
    Candidate,
    Selected,
    Published,
    Eliminated,
    DownloadFailed,
}

impl AssetStatus {
    /// 是否允许 from -> to 的合法状态迁移。
    #[allow(clippy::match_like_matches_macro)] // 成对状态矩阵用 match 更可读
    pub fn can_transition_to(&self, to: &AssetStatus) -> bool {
        use AssetStatus::*;
        match (self, to) {
            (New, Analyzing | Eliminated | DownloadFailed) => true,
            (Analyzing, New | Candidate | Eliminated | DownloadFailed) => true,
            (Candidate, Analyzing | Selected | Eliminated) => true,
            (Selected, Candidate | Published | Eliminated) => true,
            (Published, Candidate | Selected) => true,
            (Eliminated, New | Candidate | Analyzing) => true,
            (DownloadFailed, New | Eliminated) => true,
            _ => false,
        }
    }

    /// 校验并返回迁移结果；非法迁移返回 `IllegalTransition`。
    pub fn check_transition(&self, to: &AssetStatus) -> Result<(), DomainError> {
        if self.can_transition_to(to) {
            Ok(())
        } else {
            Err(DomainError::IllegalTransition {
                from: self.to_string(),
                to: to.to_string(),
            })
        }
    }
}

impl std::fmt::Display for AssetStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AssetStatus::New => "NEW",
            AssetStatus::Analyzing => "ANALYZING",
            AssetStatus::Candidate => "CANDIDATE",
            AssetStatus::Selected => "SELECTED",
            AssetStatus::Published => "PUBLISHED",
            AssetStatus::Eliminated => "ELIMINATED",
            AssetStatus::DownloadFailed => "DOWNLOAD_FAILED",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for AssetStatus {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "NEW" => Ok(AssetStatus::New),
            "ANALYZING" => Ok(AssetStatus::Analyzing),
            "CANDIDATE" => Ok(AssetStatus::Candidate),
            "SELECTED" => Ok(AssetStatus::Selected),
            "PUBLISHED" => Ok(AssetStatus::Published),
            "ELIMINATED" => Ok(AssetStatus::Eliminated),
            "DOWNLOAD_FAILED" => Ok(AssetStatus::DownloadFailed),
            other => Err(DomainError::InvalidData(format!("unknown asset status: {other}"))),
        }
    }
}

/// 素材已合成音乐的信息（video_assets.music_info_json 反序列化；NULL = 未合成过）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MusicInfo {
    /// 首次合成前的原始视频路径（再次合成从它重来，不叠音；删除素材时一并清理）。
    pub origin_file_path: String,
    /// 选用的音乐文件名（展示用，如 `Lemon - Kenshi Yonezu.m4a`）。
    pub music_name: String,
    /// 是否关闭原声。
    pub mute_original: bool,
    /// 原声音量百分比 0-100（保留原声混音时生效；旧记录缺省 30）。
    #[serde(default = "default_full_volume")]
    pub video_volume: i64,
    /// 音乐音量百分比 0-100（保留原声混音时生效；旧记录缺省 70）。
    #[serde(default = "default_bgm_volume")]
    pub music_volume: i64,
}

fn default_full_volume() -> i64 {
    30
}

fn default_bgm_volume() -> i64 {
    70
}

/// 完整的视频资产记录（含来自分析结果的分组/评分字段）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VideoAsset {
    pub id: i64,
    pub dedup_key: Option<String>,
    pub file_path: String,
    pub cover_path: Option<String>,
    pub source_platform: String,
    pub source_url: Option<String>,
    pub author: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub published_at: Option<String>,
    pub tags: Vec<String>,
    pub likes: i64,
    pub comments: i64,
    pub plays: i64,
    pub duration_ms: i64,
    pub status: AssetStatus,
    /// 淘汰原因（AI 淘汰为具体原因，手工淘汰为「手工淘汰」，非淘汰态为 None）。
    pub eliminate_reason: Option<String>,
    /// 已合成音乐的信息（未合成为 None）。
    pub music_info: Option<MusicInfo>,
    /// 来自 analysis_results 的联表字段，未分析时为 None。
    pub category: Option<String>,
    pub quality_score: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

/// 新增资产的输入数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAsset {
    pub dedup_key: Option<String>,
    pub file_path: String,
    pub cover_path: Option<String>,
    pub source_platform: String,
    pub source_url: Option<String>,
    pub author: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub published_at: Option<String>,
    pub tags: Vec<String>,
    pub likes: i64,
    pub comments: i64,
    pub plays: i64,
    pub duration_ms: i64,
}
