use serde::{Deserialize, Serialize};

use super::error::DomainError;

/// 发布草稿生命周期状态。
///
/// DRAFT：已生成文案，等待浏览器填表；
/// FILLED：表单已由 sidecar 自动填写完成，等待人工在浏览器中核对并点击发布；
/// PUBLISHED：用户在浏览器中点击发布成功；
/// CANCELLED：用户取消该草稿。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DraftStatus {
    Draft,
    Filled,
    Published,
    Cancelled,
}

impl DraftStatus {
    pub fn can_transition_to(&self, to: &DraftStatus) -> bool {
        use DraftStatus::*;
        matches!(
            (self, to),
            (Draft, Filled | Published | Cancelled) | (Filled, Published | Cancelled)
        )
    }

    pub fn check_transition(&self, to: &DraftStatus) -> Result<(), DomainError> {
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

impl std::fmt::Display for DraftStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            DraftStatus::Draft => "DRAFT",
            DraftStatus::Filled => "FILLED",
            DraftStatus::Published => "PUBLISHED",
            DraftStatus::Cancelled => "CANCELLED",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for DraftStatus {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "DRAFT" => Ok(DraftStatus::Draft),
            "FILLED" => Ok(DraftStatus::Filled),
            "PUBLISHED" => Ok(DraftStatus::Published),
            "CANCELLED" => Ok(DraftStatus::Cancelled),
            other => Err(DomainError::InvalidData(format!(
                "unknown draft status: {other}"
            ))),
        }
    }
}

/// 新建发布草稿的输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDraft {
    pub asset_id: i64,
    pub style_id: Option<i64>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub hashtags: Vec<String>,
}

/// 发布草稿实体。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishDraft {
    pub id: i64,
    pub asset_id: i64,
    pub style_id: Option<i64>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub hashtags: Vec<String>,
    pub status: DraftStatus,
    pub created_at: String,
    pub updated_at: String,
}
