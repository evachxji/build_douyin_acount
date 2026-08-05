use std::path::Path;

use crate::domain::error::DomainError;

use super::pipeline::FrameInsight;
use super::AiClient;

const FRAME_PROMPT: &str = include_str!("../../../resources/prompts/vision-frame.md");

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
        let value: serde_json::Value = serde_json::from_str(&raw)
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
}
