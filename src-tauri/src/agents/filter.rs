use crate::agents::{Agent, AgentContext, AgentOutcome};
use crate::db::repos::analyses::AnalysesRepo;
use crate::db::repos::assets::AssetsRepo;
use crate::domain::analysis::{FilterVerdict, NewAnalysis};
use crate::domain::asset::AssetStatus;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;

/// 过滤规则阈值（集中配置，后续可迁移到 resources/filter-rules.toml）。
#[derive(Debug, Clone)]
pub struct FilterRules {
    /// ASR 文本中英文字符词占比超过该值即判定英文主导。
    pub english_ratio_threshold: f64,
    /// 低于该质量分进入低优先级。
    pub quality_min_keep: i64,
}

impl Default for FilterRules {
    fn default() -> Self {
        Self {
            english_ratio_threshold: 0.6,
            quality_min_keep: 60,
        }
    }
}

/// 计算文本中"纯拉丁字母词"占比，用于英文对白占比的启发式判定。
pub fn english_word_ratio(text: &str) -> f64 {
    let words: Vec<&str> = text.split_whitespace().filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return 0.0;
    }
    let english = words
        .iter()
        .filter(|w| w.chars().all(|c| c.is_ascii_alphabetic()))
        .count();
    english as f64 / words.len() as f64
}

/// Filter Agent：英文/质量过滤判定（纯逻辑，可独立测试）。
pub struct FilterAgent {
    rules: FilterRules,
}

impl FilterAgent {
    pub fn new(rules: FilterRules) -> Self {
        Self { rules }
    }

    /// 依据分析结果判定保留/淘汰。
    pub fn evaluate(analysis: &NewAnalysis, rules: &FilterRules) -> Result<FilterVerdict, DomainError> {
        if analysis.has_large_amount_english_subtitle {
            return Ok(FilterVerdict::SkipDownload);
        }
        if analysis.has_foreign_language {
            let ratio = analysis
                .asr_text
                .as_deref()
                .map(english_word_ratio)
                .unwrap_or(0.0);
            if ratio >= rules.english_ratio_threshold {
                return Ok(FilterVerdict::SkipDownload);
            }
        }
        if let Some(score) = analysis.quality_score {
            if score < rules.quality_min_keep {
                return Ok(FilterVerdict::LowPriority);
            }
        }
        Ok(FilterVerdict::Keep)
    }

    /// 将判定结果映射为资产状态：保留 -> 候选；其余（英文主导/质量不佳）统一淘汰。
    pub fn verdict_status(verdict: FilterVerdict) -> AssetStatus {
        match verdict {
            FilterVerdict::Keep => AssetStatus::Candidate,
            FilterVerdict::SkipDownload | FilterVerdict::LowPriority => AssetStatus::Eliminated,
        }
    }
}

impl Agent for FilterAgent {
    fn name(&self) -> &'static str {
        "filter"
    }

    fn run(&self, ctx: &AgentContext, task: AgentTask) -> Result<AgentOutcome, DomainError> {
        let asset_id = task.payload["asset_id"]
            .as_i64()
            .ok_or_else(|| DomainError::InvalidData("payload missing asset_id".to_string()))?;
        let analysis = AnalysesRepo::new(ctx.db)
            .get_by_asset(asset_id)?
            .ok_or(DomainError::InvalidData(format!("analysis not found for asset {asset_id}")))?;
        let new_analysis = NewAnalysis {
            asset_id: analysis.asset_id,
            category: analysis.category,
            tags: analysis.tags,
            quality_score: analysis.quality_score,
            has_foreign_language: analysis.has_foreign_language,
            has_large_amount_english_subtitle: analysis.has_large_amount_english_subtitle,
            summary: analysis.summary,
            recommended_caption: analysis.recommended_caption,
            asr_text: analysis.asr_text,
            asr_language: analysis.asr_language,
            frame_paths: analysis.frame_paths,
            model_name: analysis.model_name,
            prompt_version: analysis.prompt_version,
        };
        let verdict = Self::evaluate(&new_analysis, &self.rules)?;
        let status = Self::verdict_status(verdict);
        AssetsRepo::new(ctx.db).update_status(asset_id, status)?;
        Ok(AgentOutcome::Succeeded(serde_json::json!({
            "asset_id": asset_id,
            "verdict": verdict.as_str(),
            "status": status.to_string(),
        })))
    }
}
