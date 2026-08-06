use serde::{Deserialize, Serialize};

/// AI 分析结果（对应 analysis_results 表）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalysisResult {
    pub id: i64,
    pub asset_id: i64,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub quality_score: Option<i64>,
    pub has_foreign_language: bool,
    pub has_large_amount_english_subtitle: bool,
    pub summary: Option<String>,
    pub recommended_caption: Option<String>,
    pub asr_text: Option<String>,
    pub asr_language: Option<String>,
    pub frame_paths: Vec<String>,
    pub model_name: Option<String>,
    pub prompt_version: Option<String>,
    #[serde(default)]
    pub condition_results: Vec<ConditionVerdict>,
    pub created_at: String,
}

/// 新增/更新分析结果的输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAnalysis {
    pub asset_id: i64,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub quality_score: Option<i64>,
    pub has_foreign_language: bool,
    pub has_large_amount_english_subtitle: bool,
    pub summary: Option<String>,
    pub recommended_caption: Option<String>,
    pub asr_text: Option<String>,
    pub asr_language: Option<String>,
    pub frame_paths: Vec<String>,
    pub model_name: Option<String>,
    pub prompt_version: Option<String>,
    #[serde(default)]
    pub condition_results: Vec<ConditionVerdict>,
}

/// 单条筛选条件的 LLM 判定结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConditionVerdict {
    pub name: String,
    pub hit: bool,
    pub basis: String,
}

/// 英文/质量过滤判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterVerdict {
    Keep,
    SkipDownload,
    LowPriority,
}

impl FilterVerdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            FilterVerdict::Keep => "keep",
            FilterVerdict::SkipDownload => "skip_download",
            FilterVerdict::LowPriority => "low_priority",
        }
    }
}
