pub mod asr;
pub mod frames;
pub mod llm;
pub mod pipeline;
pub mod vision;

pub use asr::{Asr, Transcript};

/// 与本地 AI 服务（Ollama / OpenAI 兼容接口）对话的 HTTP 客户端。
pub struct AiClient {
    base_url: String,
    http: reqwest::Client,
}

/// 聊天消息（与 Ollama /api/chat 协议兼容）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            images: Vec::new(),
        }
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
            images: Vec::new(),
        }
    }
}

impl AiClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self, crate::domain::error::DomainError> {
        let base_url = base_url.into();
        // 启动时尽早校验 URL 格式，避免运行时才发现配置错误。
        reqwest::Url::parse(&base_url).map_err(|e| {
            crate::domain::error::DomainError::InvalidData(format!("invalid AI base_url: {e}"))
        })?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                // 本地 AI 服务不走系统/环境代理。
                .no_proxy()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .map_err(|e| crate::domain::error::DomainError::InvalidData(format!("http client: {e}")))?,
        })
    }

    /// 文本对话，返回 assistant 的 content 原文。
    pub async fn chat(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
        format: Option<&str>,
    ) -> Result<String, crate::domain::error::DomainError> {
        let mut payload = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": false,
        });
        if let Some(f) = format {
            payload["format"] = serde_json::Value::String(f.to_string());
        }
        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| crate::domain::error::DomainError::InvalidData(format!("ai request failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(crate::domain::error::DomainError::Http {
                status: status.as_u16(),
                body,
            });
        }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| crate::domain::error::DomainError::InvalidData(format!("ai response parse failed: {e}")))?;
        json["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| {
                crate::domain::error::DomainError::InvalidData("ai response missing message.content".to_string())
            })
    }

    /// 带图片的对话（Ollama Vision）。
    pub async fn chat_with_images(
        &self,
        model: &str,
        prompt: &str,
        images: Vec<&std::path::Path>,
        format: Option<&str>,
    ) -> Result<String, crate::domain::error::DomainError> {
        let mut images_b64 = Vec::with_capacity(images.len());
        for p in images {
            images_b64.push(crate::util::file_to_base64(p)?);
        }
        let message = ChatMessage {
            role: "user".to_string(),
            content: prompt.to_string(),
            images: images_b64,
        };
        self.chat(model, vec![message], format).await
    }

    /// 探测服务是否可用（用于 GUI 显示连接状态）。
    pub async fn is_available(&self) -> bool {
        self.http
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}
