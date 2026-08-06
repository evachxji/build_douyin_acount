use crate::agents::{Agent, AgentContext, AgentOutcome};
use crate::db::repos::analyses::AnalysesRepo;
use crate::db::repos::assets::AssetsRepo;
use crate::domain::analysis::{FilterVerdict, NewAnalysis};
use crate::domain::asset::AssetStatus;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;

/// 过滤规则与自定义条件（由设置注入）。
#[derive(Debug, Clone)]
pub struct FilterRules {
    /// 低于该质量分进入低优先级。
    pub quality_min_keep: i64,
    /// 命中条件：任一命中即通过；空 = 不限制。
    pub hit_conditions: Vec<String>,
    /// 排除条件：任一命中即淘汰；空 = 不限制。
    pub exclude_conditions: Vec<String>,
}

impl Default for FilterRules {
    fn default() -> Self {
        Self {
            quality_min_keep: 60,
            hit_conditions: Vec::new(),
            exclude_conditions: Vec::new(),
        }
    }
}

/// 过滤判定结果：结论 + 淘汰原因（供任务日志排查）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterDecision {
    pub verdict: FilterVerdict,
    pub reason: Option<String>,
}

/// 查询某条件在分析判定中的命中情况（未判定返回 None）。
fn verdict_of(analysis: &NewAnalysis, name: &str) -> Option<bool> {
    analysis
        .condition_results
        .iter()
        .find(|v| v.name == name)
        .map(|v| v.hit)
}

/// 命中/排除条件与质量分的判定实现（纯逻辑，可独立测试）。
fn decide(analysis: &NewAnalysis, rules: &FilterRules) -> FilterDecision {
    // 排除条件层：任一已判定命中即淘汰（未判定条目不构成排除）。
    for c in rules
        .exclude_conditions
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        if verdict_of(analysis, c) == Some(true) {
            return FilterDecision {
                verdict: FilterVerdict::SkipDownload,
                reason: Some(format!("excluded_by_condition: {c}")),
            };
        }
    }
    // 命中条件层：全部已判定且全部未命中才淘汰；存在未判定条目则跳过本层，避免误杀。
    let hits: Vec<&str> = rules
        .hit_conditions
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if !hits.is_empty()
        && hits.iter().all(|c| verdict_of(analysis, c).is_some())
        && hits.iter().all(|c| verdict_of(analysis, c) == Some(false))
    {
        return FilterDecision {
            verdict: FilterVerdict::SkipDownload,
            reason: Some("no_hit_condition".to_string()),
        };
    }
    // 质量分规则（保留）。
    if let Some(score) = analysis.quality_score {
        if score < rules.quality_min_keep {
            return FilterDecision {
                verdict: FilterVerdict::LowPriority,
                reason: None,
            };
        }
    }
    FilterDecision { verdict: FilterVerdict::Keep, reason: None }
}

/// Filter Agent：过滤判定（纯逻辑，可独立测试）。
pub struct FilterAgent {
    rules: FilterRules,
}

impl FilterAgent {
    pub fn new(rules: FilterRules) -> Self {
        Self { rules }
    }

    pub fn evaluate(
        analysis: &NewAnalysis,
        rules: &FilterRules,
    ) -> Result<FilterDecision, DomainError> {
        Ok(decide(analysis, rules))
    }

    /// 将判定结果映射为资产状态：保留 -> 候选；其余（条件淘汰/质量不佳）统一淘汰。
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
            condition_results: analysis.condition_results,
        };
        let decision = Self::evaluate(&new_analysis, &self.rules)?;
        let status = Self::verdict_status(decision.verdict);
        AssetsRepo::new(ctx.db).update_status(asset_id, status)?;
        Ok(AgentOutcome::Succeeded(serde_json::json!({
            "asset_id": asset_id,
            "verdict": decision.verdict.as_str(),
            "status": status.to_string(),
            "reason": decision.reason,
        })))
    }
}
