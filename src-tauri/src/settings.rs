use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::domain::error::DomainError;

/// AI / 过滤 / 存储等应用级配置。集中在此，GUI 可读可改并持久化到 app_settings 表。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppSettings {
    pub ollama_url: String,
    pub vision_model: String,
    pub llm_model: String,
    pub whisper_python: String,
    pub whisper_script: String,
    pub whisper_model: String,
    pub english_ratio_threshold: f64,
    pub quality_min_keep: i64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ollama_url: "http://localhost:11434".to_string(),
            vision_model: "qwen2.5vl:7b".to_string(),
            llm_model: "qwen2.5:7b".to_string(),
            whisper_python: "scripts/whisper-server/.venv/Scripts/python.exe".to_string(),
            whisper_script: "scripts/whisper-server/whisper_transcribe.py".to_string(),
            whisper_model: "base".to_string(),
            english_ratio_threshold: 0.6,
            quality_min_keep: 60,
        }
    }
}

/// app_settings 表访问层：整个设置序列化为 JSON 存在 `app` 键下。
pub struct SettingsRepo<'a> {
    db: &'a Database,
}

impl<'a> SettingsRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// 读取设置；从未保存或内容损坏时回退默认值。
    pub fn load(&self) -> Result<AppSettings, DomainError> {
        let raw: Option<String> = self
            .db
            .conn()
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'app'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let Some(raw) = raw else {
            return Ok(AppSettings::default());
        };
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    /// 保存设置（upsert）。
    pub fn save(&self, settings: &AppSettings) -> Result<(), DomainError> {
        let raw = serde_json::to_string(settings)?;
        self.db.conn().execute(
            "INSERT INTO app_settings (key, value) VALUES ('app', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                updated_at = datetime('now')",
            [&raw],
        )?;
        Ok(())
    }
}
