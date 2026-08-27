use std::time::Duration;

use crate::domain::error::{ApiError, DomainError};

/// 拉取云端 OpenAI 兼容供应商的可用模型列表（GET {base_url}/models）。
/// DeepSeek / Kimi Coding Plan / OpenRouter 三家均实现该端点（2026-08 实测：前两家无 key 401，
/// OpenRouter 公开可拉）。api_key 为空时不带 Bearer 头，让供应商自己决定放行或 401 原样上报。
#[tauri::command]
pub async fn list_cloud_models(base_url: String, api_key: String) -> Result<Vec<String>, ApiError> {
    fetch_cloud_models(&base_url, &api_key)
        .await
        .map_err(ApiError::from)
}

pub async fn fetch_cloud_models(
    base_url: &str,
    api_key: &str,
) -> Result<Vec<String>, DomainError> {
    let base = base_url.trim_end_matches('/');
    reqwest::Url::parse(base).map_err(|e| DomainError::InvalidData(format!("invalid base_url: {e}")))?;
    // 云端服务走系统/环境代理（与 OpenAiClient 一致，不配 no_proxy）。
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| DomainError::InvalidData(format!("http client: {e}")))?;
    let mut req = client.get(format!("{base}/models"));
    if !api_key.trim().is_empty() {
        req = req.bearer_auth(api_key.trim());
    }
    let resp = req
        .send()
        .await
        .map_err(|e| DomainError::InvalidData(format!("cloud models request failed: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        let mut body = resp.text().await.unwrap_or_default();
        body.truncate(300);
        return Err(DomainError::Http {
            status: status.as_u16(),
            body,
        });
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| DomainError::InvalidData(format!("cloud models response parse failed: {e}")))?;
    let mut ids: Vec<String> = json["data"]
        .as_array()
        .ok_or_else(|| DomainError::InvalidData("cloud models response missing data[]".to_string()))?
        .iter()
        .filter_map(|m| m["id"].as_str().map(str::to_string))
        .collect();
    ids.sort();
    Ok(ids)
}
