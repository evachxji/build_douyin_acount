use crate::agents::filter::{FilterAgent, FilterRules};
use crate::agents::{Agent, AgentContext, AgentOutcome};
use crate::analysis::pipeline::AnalysisPipeline;
use crate::db::repos::analyses::AnalysesRepo;
use crate::db::repos::assets::AssetsRepo;
use crate::domain::asset::AssetStatus;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;

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

        let work_dir = ctx.work_dir(asset_id);
        let analysis = self
            .pipeline
            .analyze(
                std::path::Path::new(&asset.file_path),
                &work_dir,
                asset_id,
                asset.title.as_deref(),
            )?;
        let decision = FilterAgent::evaluate(&analysis, &self.rules)?;
        AnalysesRepo::new(ctx.db).upsert(analysis)?;
        let status = FilterAgent::verdict_status(decision.verdict);
        assets.update_status(asset_id, status)?;
        Ok(AgentOutcome::Succeeded(serde_json::json!({
            "asset_id": asset_id,
            "verdict": decision.verdict.as_str(),
            "status": status.to_string(),
            "reason": decision.reason,
        })))
    }
}
