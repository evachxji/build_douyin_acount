use std::path::Path;

use crate::domain::error::DomainError;

use super::openai::OpenAiClient;
use super::pipeline::FrameInsight;
use super::AiClient;

const FRAME_PROMPT: &str = include_str!("../../../resources/prompts/vision-frame.md");

/// 解析帧分析 JSON 输出（本地/云端实现共用）。
fn parse_frame_insight(raw: &str) -> Result<FrameInsight, DomainError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| DomainError::InvalidData(format!("frame insight invalid json: {e}")))?;
    let description = value
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Ok(FrameInsight {
        description,
        english_text: value
            .get("english_text")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    })
}

/// 基于 Ollama Vision 的帧分析实现。
pub struct OllamaVision {
    pub client: AiClient,
    pub model: String,
}

impl super::pipeline::Vision for OllamaVision {
    fn analyze_frame(&self, path: &Path) -> Result<FrameInsight, DomainError> {
        let rt = tokio::runtime::Runtime::new().map_err(|e| DomainError::InvalidData(format!("rt: {e}")))?;
        let raw = rt.block_on(async {
            self.client
                .chat_with_images(&self.model, FRAME_PROMPT, vec![path], Some("json"))
                .await
        })?;
        parse_frame_insight(&raw)
    }
}

/// 基于 OpenAI 兼容云端（Kimi 等）的帧分析实现。
pub struct OpenAiVision {
    pub client: OpenAiClient,
    pub model: String,
}

impl super::pipeline::Vision for OpenAiVision {
    fn analyze_frame(&self, path: &Path) -> Result<FrameInsight, DomainError> {
        let rt = tokio::runtime::Runtime::new().map_err(|e| DomainError::InvalidData(format!("rt: {e}")))?;
        let raw = rt.block_on(async {
            self.client
                .chat_with_image(&self.model, FRAME_PROMPT, path, true)
                .await
        })?;
        parse_frame_insight(&raw)
    }

    /// 云端无本地算力约束，多帧并发分析。
    fn prefer_concurrency(&self) -> bool {
        true
    }
}
