use std::sync::Arc;

use serde_json::json;

use crate::agents::{Agent, AgentContext, AgentOutcome};
use crate::analysis::{AiClient, ChatMessage};
use crate::db::repos::analyses::AnalysesRepo;
use crate::db::repos::content::{ContentRepo, StylesRepo};
use crate::domain::content::NewContentCopy;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;

/// 文案生成的 LLM 依赖（便于测试注入假实现）。
pub trait CopyGenerator: Send + Sync {
    fn generate_copy(&self, style_prompt: &str, analysis_context: &str) -> Result<String, DomainError>;
}

/// 基于 Ollama 的实现：同步包装异步 chat（与 Vision 管线一致）。
pub struct OllamaCopyGenerator {
    pub client: AiClient,
    pub model: String,
}

impl CopyGenerator for OllamaCopyGenerator {
    fn generate_copy(&self, style_prompt: &str, analysis_context: &str) -> Result<String, DomainError> {
        let system = format!(
            "你是抖音短视频运营文案专家。账号风格要求：{style_prompt}。\n\
             只输出 JSON，不要任何多余文字：{{\"title\":\"抖音标题（≤30字，可带 emoji）\",\"body\":\"正文描述（1~3句）\",\"hashtags\":[\"话题1\",\"话题2\"]}}"
        );
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| DomainError::InvalidData(format!("tokio runtime: {e}")))?;
        let raw = rt.block_on(async {
            self.client
                .chat(
                    &self.model,
                    vec![ChatMessage::system(system), ChatMessage::user(analysis_context)],
                    Some("json"),
                )
                .await
        })?;
        Ok(raw)
    }
}

/// Content Agent：为单个素材按账号风格生成抖音标题/正文/话题。
pub struct ContentAgent {
    llm: Arc<dyn CopyGenerator>,
}

impl ContentAgent {
    pub fn new(llm: Arc<dyn CopyGenerator>) -> Self {
        Self { llm }
    }
}

/// 解析并校验 LLM 输出的文案 JSON。
fn parse_copy(raw: &str) -> Result<(String, String, Vec<String>), DomainError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| DomainError::InvalidData(format!("copy json invalid: {e}")))?;
    let title: String = value
        .get("title")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| DomainError::InvalidData("copy missing title".to_string()))?;
    let body: String = value
        .get("body")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let hashtags: Vec<String> = value
        .get("hashtags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str().map(|s| s.trim_matches('#').trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    // 抖音标题上限 30 字符
    let title: String = title.chars().take(30).collect();
    Ok((title, body, hashtags))
}

fn analysis_context(asset_id: i64, ctx: &AgentContext) -> Result<String, DomainError> {
    let analysis = AnalysesRepo::new(ctx.db)
        .get_by_asset(asset_id)?
        .ok_or_else(|| DomainError::InvalidData("素材尚未分析，请先执行 AI 分析".to_string()))?;
    let asset = crate::db::repos::assets::AssetsRepo::new(ctx.db)
        .get(asset_id)?
        .ok_or(DomainError::AssetNotFound(asset_id))?;
    Ok(format!(
        "素材标题：{}\n分类：{}\n标签：{}\n质量分：{}\nAI 总结：{}\n音频转写：{}\n原始视频标题：{}",
        asset.title.as_deref().unwrap_or("未知"),
        analysis.category.as_deref().unwrap_or("未分类"),
        analysis.tags.join("、"),
        analysis.quality_score.map(|s| s.to_string()).unwrap_or_else(|| "-".into()),
        analysis.summary.as_deref().unwrap_or("无"),
        analysis.asr_text.as_deref().unwrap_or("无"),
        asset.title.as_deref().unwrap_or(""),
    ))
}

impl Agent for ContentAgent {
    fn name(&self) -> &'static str {
        "content"
    }

    fn run(&self, ctx: &AgentContext, task: AgentTask) -> Result<AgentOutcome, DomainError> {
        let asset_id = task.payload["asset_id"]
            .as_i64()
            .ok_or_else(|| DomainError::InvalidData("payload missing asset_id".to_string()))?;
        let style_name = task.payload["style"].as_str().unwrap_or("治愈");
        let style = StylesRepo::new(ctx.db)
            .get_by_name(style_name)?
            .ok_or_else(|| DomainError::InvalidData(format!("未知账号风格：{style_name}")))?;

        let context = analysis_context(asset_id, ctx)?;
        let raw = self.llm.generate_copy(&style.tone_prompt, &context)?;
        let (title, body, hashtags) = parse_copy(&raw)?;
        ContentRepo::new(ctx.db).upsert(NewContentCopy {
            asset_id,
            style_id: Some(style.id),
            title,
            body,
            hashtags,
        })?;
        Ok(AgentOutcome::Succeeded(json!({
            "asset_id": asset_id,
            "style": style_name,
        })))
    }
}
