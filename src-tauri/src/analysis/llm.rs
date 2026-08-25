use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::domain::analysis::{ConditionBasis, ConditionVerdict};
use crate::domain::error::DomainError;

use super::openai::OpenAiClient;
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
    #[serde(default)]
    pub conditions: Vec<IndexVerdict>,
}

/// 模型按编号输出的条件判定（index 对应筛选要求中的编号）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexVerdict {
    pub index: usize,
    pub hit: bool,
    pub basis: String,
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
    let conditions = value
        .get("conditions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    let index = c.get("index")?.as_u64()? as usize;
                    Some(IndexVerdict {
                        index,
                        hit: c.get("hit").and_then(|v| v.as_bool()).unwrap_or(false),
                        basis: c.get("basis").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
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
        conditions,
    })
}

/// 把模型按编号输出的判定映射回 `names`（调用方给定的完整编号顺序）。
/// 编号越界、重复编号、空条件名一律跳过，保证与设置列表精确对应。
pub fn resolve_conditions(raw: &[IndexVerdict], names: &[String]) -> Vec<ConditionVerdict> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for v in raw {
        if v.index == 0 || v.index > names.len() || !seen.insert(v.index) {
            continue;
        }
        let name = names[v.index - 1].as_str();
        if name.trim().is_empty() {
            continue;
        }
        out.push(ConditionVerdict {
            name: name.to_string(),
            hit: v.hit,
            basis: v.basis.clone(),
        });
    }
    out
}

/// 解析模型返回的 `{"conditions": [{"index":..,"hit":..}]}` 为编号判定列表（容错）。
fn parse_index_verdicts(raw: &str) -> Result<Vec<IndexVerdict>, DomainError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| DomainError::InvalidData(format!("judge json invalid: {e}")))?;
    let arr = value
        .get("conditions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| DomainError::InvalidData("judge json missing conditions".to_string()))?;
    Ok(arr
        .iter()
        .filter_map(|c| {
            Some(IndexVerdict {
                index: c.get("index")?.as_u64()? as usize,
                hit: c.get("hit").and_then(|v| v.as_bool()).unwrap_or(false),
                basis: String::new(),
            })
        })
        .collect())
}

/// 组装条件分类的 system/user 提示词（本地/云端实现共用）。
fn build_classify_prompt(conditions: &[String]) -> (&'static str, String) {
    let list = conditions
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {c}", i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    let user = format!(
        "以下是短视频筛选条件清单：\n{list}\n\n请判断每一条条件主要依据哪种内容判断是否命中：\n「画面」：只能或主要靠视频画面判断（如 色情、大量英文字幕、宠物、风景）\n「音频」：只能或主要靠语音/转写判断（如 长中文对话、长英文对话、旁白解说）\n「不限」：画面或音频都可以判断（如 搞笑、治愈）\n输出严格 JSON：{{\"conditions\": [{{\"index\": 编号, \"basis\": \"画面|音频|不限\"}}]}}"
    );
    ("你是一个内容筛选条件分类助手。只输出 JSON，不要输出其他文字。", user)
}

/// 解析条件分类响应并映射回条件名（本地/云端实现共用）。
fn parse_classify_response(raw: &str, conditions: &[String]) -> Result<HashMap<String, ConditionBasis>, DomainError> {
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| DomainError::InvalidData(format!("classify json invalid: {e}")))?;
    let mut out = HashMap::new();
    for c in value.get("conditions").and_then(|v| v.as_array()).into_iter().flatten() {
        let (Some(index), Some(basis)) = (
            c.get("index").and_then(|v| v.as_u64()),
            c.get("basis").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        let name = conditions.get(index as usize - 1);
        let basis = match basis {
            "画面" => ConditionBasis::Visual,
            "音频" => ConditionBasis::Audio,
            _ => ConditionBasis::Any,
        };
        if let Some(name) = name {
            out.insert(name.clone(), basis);
        }
    }
    Ok(out)
}

/// 让 LLM 自动判断每个筛选条件主要依据「画面 / 音频 / 不限」哪种内容判定。
pub fn classify_condition_bases(
    client: &AiClient,
    model: &str,
    conditions: &[String],
) -> Result<HashMap<String, ConditionBasis>, DomainError> {
    if conditions.is_empty() {
        return Ok(HashMap::new());
    }
    let (system, user) = build_classify_prompt(conditions);
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| DomainError::InvalidData(format!("tokio runtime: {e}")))?;
    let raw = rt.block_on(async {
        client
            .chat(
                model,
                vec![super::ChatMessage::system(system), super::ChatMessage::user(user)],
                Some("json"),
            )
            .await
    })?;
    parse_classify_response(&raw, conditions)
}

/// 基于 Ollama 的 LLM 综合实现。
pub struct OllamaLlm {
    pub client: AiClient,
    pub model: String,
}

/// 组装 LLM 综合判定的 system/user 提示词（本地/云端实现共用）。
fn build_synthesize_prompt(req: &SynthesisRequest) -> (&'static str, String) {
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
    let mut filter_lines = Vec::new();
    for (i, c) in &req.conditions {
        filter_lines.push(format!("{i}. {c}"));
    }
    let filter_section = if filter_lines.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n筛选要求（按编号逐条判定；conditions 中每条 index 必须与编号一致）：\n{}",
            filter_lines.join("\n")
        )
    };
    let user = format!(
        "视频元信息：\n标题：{}\n\n关键帧内容：\n{frames_text}\n\n音频转写（语言：{}）：\n{}{filter_section}",
        req.title.as_deref().unwrap_or("未知"),
        req.asr_language.as_deref().unwrap_or("unknown"),
        asr_text
    );
    (SYNTHESIZE_PROMPT, user)
}

/// 组装音频类条件预检的 system/user 提示词（本地/云端实现共用）。
fn build_judge_prompt(
    title: Option<&str>,
    asr_text: &str,
    asr_language: Option<&str>,
    items: &[(usize, String)],
) -> (&'static str, String) {
    let lines = items
        .iter()
        .map(|(i, n)| format!("{i}. {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let user = format!(
        "视频标题：{}\n音频转写（语言：{}）：\n{}\n\n请仅依据上述标题与音频转写逐条判定以下条件是否命中（不要推测画面内容）：\n{lines}\n\n注意：音频转写可能混有背景音乐歌词。若条件针对对话/旁白/说话内容（如 长对话、非中文对话、英文对话），明显的歌词/吟唱（押韵、重复、片段化、旋律性）不计入，一律判定未命中；若条件针对歌曲/音乐本身（如 英文歌曲、BGM），则照常依据转写判定。\n\n输出严格 JSON：{{\"conditions\": [{{\"index\": 编号, \"hit\": true或false}}]}}",
        title.unwrap_or("未知"),
        asr_language.unwrap_or("unknown"),
        asr_text
    );
    ("你是一个短视频内容筛选助手。只输出 JSON，不要输出其他文字。", user)
}

impl super::pipeline::Llm for OllamaLlm {
    fn synthesize(&self, req: SynthesisRequest) -> Result<AnalysisJson, DomainError> {
        let (system, user) = build_synthesize_prompt(&req);
        // 当前为同步 trait：阻塞式调用本地 HTTP 服务。
        let rt = tokio::runtime::Runtime::new().map_err(|e| DomainError::InvalidData(format!("rt: {e}")))?;
        let raw = rt.block_on(async {
            self.client
                .chat(
                    &self.model,
                    vec![super::ChatMessage::system(system), super::ChatMessage::user(user)],
                    Some("json"),
                )
                .await
        })?;
        parse_analysis_json(&raw)
    }

    fn judge_conditions(
        &self,
        title: Option<&str>,
        asr_text: &str,
        asr_language: Option<&str>,
        items: &[(usize, String)],
    ) -> Result<Vec<IndexVerdict>, DomainError> {
        let (system, user) = build_judge_prompt(title, asr_text, asr_language, items);
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| DomainError::InvalidData(format!("tokio runtime: {e}")))?;
        let raw = rt.block_on(async {
            self.client
                .chat(
                    &self.model,
                    vec![super::ChatMessage::system(system), super::ChatMessage::user(user)],
                    Some("json"),
                )
                .await
        })?;
        parse_index_verdicts(&raw)
    }

    fn classify_condition_bases(
        &self,
        conditions: &[String],
    ) -> Result<HashMap<String, ConditionBasis>, DomainError> {
        classify_condition_bases(&self.client, &self.model, conditions)
    }
}

/// 基于 OpenAI 兼容云端（Kimi 等）的 LLM 综合实现。
pub struct OpenAiLlm {
    pub client: OpenAiClient,
    pub model: String,
}

impl super::pipeline::Llm for OpenAiLlm {
    fn synthesize(&self, req: SynthesisRequest) -> Result<AnalysisJson, DomainError> {
        let (system, user) = build_synthesize_prompt(&req);
        let rt = tokio::runtime::Runtime::new().map_err(|e| DomainError::InvalidData(format!("rt: {e}")))?;
        let raw = rt.block_on(self.client.chat(&self.model, system, &user, true))?;
        parse_analysis_json(&raw)
    }

    fn judge_conditions(
        &self,
        title: Option<&str>,
        asr_text: &str,
        asr_language: Option<&str>,
        items: &[(usize, String)],
    ) -> Result<Vec<IndexVerdict>, DomainError> {
        let (system, user) = build_judge_prompt(title, asr_text, asr_language, items);
        let rt = tokio::runtime::Runtime::new().map_err(|e| DomainError::InvalidData(format!("rt: {e}")))?;
        let raw = rt.block_on(self.client.chat(&self.model, system, &user, true))?;
        parse_index_verdicts(&raw)
    }

    fn classify_condition_bases(
        &self,
        conditions: &[String],
    ) -> Result<HashMap<String, ConditionBasis>, DomainError> {
        if conditions.is_empty() {
            return Ok(HashMap::new());
        }
        let (system, user) = build_classify_prompt(conditions);
        let rt = tokio::runtime::Runtime::new().map_err(|e| DomainError::InvalidData(format!("rt: {e}")))?;
        let raw = rt.block_on(self.client.chat(&self.model, system, &user, true))?;
        parse_classify_response(&raw, conditions)
    }
}

fn describe_frame(f: &FrameInsight) -> String {
    format!(
        "画面：{}；包含英文字幕：{}",
        f.description,
        if f.english_text { "是" } else { "否" }
    )
}
