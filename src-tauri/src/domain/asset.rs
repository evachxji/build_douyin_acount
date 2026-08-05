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
            (Published, Candidate) => true,
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
    pub is_favorite: bool,
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
