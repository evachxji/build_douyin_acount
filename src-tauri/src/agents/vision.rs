use crate::agents::filter::{FilterAgent, FilterRules};
use crate::agents::{Agent, AgentContext, AgentOutcome};
use crate::analysis::pipeline::AnalysisPipeline;
use crate::db::repos::analyses::AnalysesRepo;
use crate::db::repos::assets::AssetsRepo;
use crate::domain::asset::AssetStatus;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;
use crate::settings::SettingsRepo;

/// Vision Agent：对单个资产执行 抽帧→ASR→Vision→LLM 综合→过滤 全流程。
pub struct VisionAgent {
    pipeline: AnalysisPipeline,
    rules: FilterRules,
}

impl VisionAgent {
    pub fn new(pipeline: AnalysisPipeline, rules: FilterRules) -> Self {
        Self { pipeline, rules }
    }
}

impl Agent for VisionAgent {
    fn name(&self) -> &'static str {
        "vision"
    }

    fn run(&self, ctx: &AgentContext, task: AgentTask) -> Result<AgentOutcome, DomainError> {
        let asset_id = task.payload["asset_id"]
            .as_i64()
            .ok_or_else(|| DomainError::InvalidData("payload missing asset_id".to_string()))?;
        let assets = AssetsRepo::new(ctx.db);
        let asset = assets
            .get(asset_id)?
            .ok_or(DomainError::AssetNotFound(asset_id))?;
        // 逐条导入已把资产置为 ANALYZING，重试时无需（也不允许）重复迁移。
        if asset.status != AssetStatus::Analyzing {
            assets.update_status(asset_id, AssetStatus::Analyzing)?;
        }

        let mut rules = self.rules.clone();
        // 惰性分类：从库中读取最新判定依据，对未知条件调用 LLM 分类并持久化，
        // 后续资产直接用分类结果做「音频类排除先判」。
        let fresh = SettingsRepo::new(ctx.db).load()?;
        rules.condition_basis = fresh.condition_basis;
        let all: Vec<String> = rules
            .hit_conditions
            .iter()
            .chain(rules.exclude_conditions.iter())
            .cloned()
            .collect();
        let unknown: Vec<String> = all
            .iter()
            .filter(|c| !rules.condition_basis.contains_key(*c))
            .cloned()
            .collect();
        if !unknown.is_empty() {
            let bases = self.pipeline.classify_condition_bases(&unknown)?;
            if !bases.is_empty() {
                let mut settings = SettingsRepo::new(ctx.db).load()?;
                for (name, basis) in &bases {
                    settings.condition_basis.insert(name.clone(), *basis);
                }
                SettingsRepo::new(ctx.db).save(&settings)?;
                rules.condition_basis.extend(bases);
            }
        }

        let work_dir = ctx.work_dir(asset_id);
        let analysis = self
            .pipeline
            .analyze(
                std::path::Path::new(&asset.file_path),
                &work_dir,
                asset_id,
                asset.title.as_deref(),
                &rules.hit_conditions,
                &rules.exclude_conditions,
                &rules.condition_basis,
            )?;
        let decision = FilterAgent::evaluate(&analysis, &rules)?;
        let reason_text = FilterAgent::eliminate_reason_text(&decision, &analysis, &rules);
        AnalysesRepo::new(ctx.db).upsert(analysis)?;
        let status = FilterAgent::verdict_status(decision.verdict);
        assets.update_status(asset_id, status)?;
        assets.set_eliminate_reason(asset_id, reason_text.as_deref())?;
        Ok(AgentOutcome::Succeeded(serde_json::json!({
            "asset_id": asset_id,
            "verdict": decision.verdict.as_str(),
            "status": status.to_string(),
            "reason": decision.reason,
        })))
    }
}
