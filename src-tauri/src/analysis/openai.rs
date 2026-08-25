use std::path::Path;

use crate::domain::error::DomainError;

/// OpenAI 兼容云端（Kimi 等）的 HTTP 客户端：POST {base}/chat/completions + Bearer 认证。
/// 与本地 AiClient（Ollama 私有协议）并列，供 Vision/Llm/文案生成的云端实现使用。
pub struct OpenAiClient {
    base_url: String,
    api_key: String,
    http: reqwest::Client,
}

impl OpenAiClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Result<Self, DomainError> {
        let base_url = base_url.into();
        let api_key = api_key.into();
        // 启动时尽早校验配置，避免运行时才发现。
        reqwest::Url::parse(&base_url)
            .map_err(|e| DomainError::InvalidData(format!("invalid openai base_url: {e}")))?;
        if api_key.trim().is_empty() {
            return Err(DomainError::InvalidData(
                "使用云端 OpenAI 兼容接口前，请先在设置页填写 API Key".to_string(),
            ));
        }
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            http: reqwest::Client::builder()
                // 云端服务允许走系统/环境代理（与本地 AiClient 的 no_proxy 相反）。
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .map_err(|e| DomainError::InvalidData(format!("http client: {e}")))?,
        })
    }

    /// 纯文本对话（system + user），返回 assistant content 原文。
    /// json_mode 时附加 response_format=json_object 约束。
    pub async fn chat(
        &self,
        model: &str,
        system: &str,
        user: &str,
        json_mode: bool,
    ) -> Result<String, DomainError> {
        let messages = serde_json::json!([
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ]);
        self.post_chat(model, messages, json_mode).await
    }

    /// 带单张图片的对话：图片读文件转 base64，以 data URL 内联进 content 数组。
    pub async fn chat_with_image(
        &self,
        model: &str,
        prompt: &str,
        image: &Path,
        json_mode: bool,
    ) -> Result<String, DomainError> {
        let b64 = crate::util::file_to_base64(image)?;
        let mime = match image
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("png") => "image/png",
            Some("webp") => "image/webp",
            _ => "image/jpeg",
        };
        let messages = serde_json::json!([{
            "role": "user",
            "content": [
                {"type": "text", "text": prompt},
                {"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{b64}")}},
            ],
        }]);
        self.post_chat(model, messages, json_mode).await
    }

    async fn post_chat(
        &self,
        model: &str,
        messages: serde_json::Value,
        json_mode: bool,
    ) -> Result<String, DomainError> {
        let mut payload = serde_json::json!({
            "model": model,
            "messages": messages,
            "max_tokens": 4096,
            "stream": false,
        });
        if json_mode {
            payload["response_format"] = serde_json::json!({"type": "json_object"});
        }
        let resp = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| DomainError::InvalidData(format!("openai request failed: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(DomainError::Http {
                status: status.as_u16(),
                body,
            });
        }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| DomainError::InvalidData(format!("openai response parse failed: {e}")))?;
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| {
                DomainError::InvalidData("openai response missing choices[0].message.content".to_string())
            })?;
        // thinking 模型（k3 系）reasoning 可能吃光 max_tokens 导致 content 为空：报明确错误而非下游 JSON 解析失败。
        if content.trim().is_empty() {
            return Err(DomainError::InvalidData(format!(
                "openai response empty content (finish_reason={})",
                json["choices"][0]["finish_reason"].as_str().unwrap_or("unknown")
            )));
        }
        Ok(content.to_string())
    }
}
