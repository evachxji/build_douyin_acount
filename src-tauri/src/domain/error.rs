use serde::{Deserialize, Serialize};

/// 领域层统一错误类型。
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("asset not found: {0}")]
    AssetNotFound(i64),
    #[error("illegal status transition: {from} -> {to}")]
    IllegalTransition { from: String, to: String },
    #[error("duplicate asset: {0}")]
    Duplicate(String),
    #[error("invalid data: {0}")]
    InvalidData(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// 可序列化的错误响应（供 Tauri 命令返回给前端）。
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl From<DomainError> for ApiError {
    fn from(e: DomainError) -> Self {
        let code = match &e {
            DomainError::AssetNotFound(_) => "ASSET_NOT_FOUND",
            DomainError::IllegalTransition { .. } => "ILLEGAL_TRANSITION",
            DomainError::Duplicate(_) => "DUPLICATE",
            DomainError::InvalidData(_) => "INVALID_DATA",
            _ => "INTERNAL",
        };
        ApiError {
            code: code.to_string(),
            message: e.to_string(),
        }
    }
}
