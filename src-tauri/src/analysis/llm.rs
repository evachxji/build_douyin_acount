use serde::{Deserialize, Serialize};

use crate::domain::error::DomainError;

use super::pipeline::{FrameInsight, SynthesisRequest};
use super::AiClient;

const SYNTHESIZE_PROMPT: &str = include_str!("../../../resources/prompts/vision-synthesize.md");

/// LLM 综合判断的结构化输出。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalysisJson {
    pub category: String,
    pub tags: Vec<String>,
    pub quality_score: i64,
    pub has_foreign_language: bool,
    pub has_large_amount_english_subtitle: bool,
    pub summary: String,
    pub recommended_caption: String,
}

/// 解析并校验 LLM 输出；缺少必要字段时返回明确错误。
pub fn parse_analysis_json(raw: &str) -> Result<AnalysisJson, DomainError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| DomainError::InvalidData(format!("analysis json invalid: {e}")))?;
    let category = value
        .get("category")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| DomainError::InvalidData("analysis json missing category".to_string()))?;
    let quality_score = value
        .get("quality_score")
        .and_then(|v| v.as_i64())
        .filter(|s| (0..=100).contains(s))
        .ok_or_else(|| DomainError::InvalidData("analysis json missing quality_score".to_string()))?;
    let tags = value
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| DomainError::InvalidData("analysis json missing tags".to_string()))?;
    let summary = value
        .get("summary")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Ok(AnalysisJson {
        category: category.to_string(),
        tags,
        quality_score,
        has_foreign_language: value
            .get("has_foreign_language")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        has_large_amount_english_subtitle: value
            .get("has_large_amount_english_subtitle")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        summary,
        recommended_caption: value
            .get("recommended_caption")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

/// 基于 Ollama 的 LLM 综合实现。
pub struct OllamaLlm {
    pub client: AiClient,
    pub model: String,
}

impl super::pipeline::Llm for OllamaLlm {
    fn synthesize(&self, req: SynthesisRequest) -> Result<AnalysisJson, DomainError> {
        let frames_text = req
            .frames
            .iter()
            .enumerate()
            .map(|(i, f)| format!("[{i}] {}", describe_frame(f)))
            .collect::<Vec<_>>()
            .join("\n");
        let asr_text = req
            .asr_text
            .as_deref()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .unwrap_or("（无对白/音频转写为空）");
        let user = format!(
            "视频元信息：\n标题：{}\n\n关键帧内容：\n{frames_text}\n\n音频转写（语言：{}）：\n{}",
            req.title.as_deref().unwrap_or("未知"),
            req.asr_language.as_deref().unwrap_or("unknown"),
            asr_text
        );
        // 当前为同步 trait：阻塞式调用本地 HTTP 服务。
        let rt = tokio::runtime::Runtime::new().map_err(|e| DomainError::InvalidData(format!("rt: {e}")))?;
        let raw = rt.block_on(async {
            self.client
                .chat(
                    &self.model,
                    vec![super::ChatMessage::system(SYNTHESIZE_PROMPT), super::ChatMessage::user(user)],
                    Some("json"),
                )
                .await
        })?;
        parse_analysis_json(&raw)
    }
}

fn describe_frame(f: &FrameInsight) -> String {
    format!(
        "画面：{}；包含英文字幕：{}",
        f.description,
        if f.english_text { "是" } else { "否" }
    )
}
