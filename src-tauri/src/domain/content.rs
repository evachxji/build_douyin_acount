use serde::{Deserialize, Serialize};

/// 账号风格（来自 account_styles 表）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountStyle {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub tone_prompt: String,
}

/// AI 生成的发布文案（标题/正文/话题）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentCopy {
    pub id: i64,
    pub asset_id: i64,
    pub style_id: Option<i64>,
    pub title: String,
    pub body: String,
    pub hashtags: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewContentCopy {
    pub asset_id: i64,
    pub style_id: Option<i64>,
    pub title: String,
    pub body: String,
    pub hashtags: Vec<String>,
}
