use std::collections::HashMap;

use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::domain::analysis::ConditionBasis;
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
    pub quality_min_keep: i64,
    /// 抖音发布 sidecar（Node 脚本路径）与浏览器通道。
    pub node_bin: String,
    pub publisher_script: String,
    pub publisher_browser: String,
    /// 采集 sidecar 与 yt-dlp 路径。
    pub collector_script: String,
    pub ytdlp_path: String,
    /// 抖音作品数据同步 sidecar（Node 脚本路径）。
    #[serde(default = "default_douyin_sync_script")]
    pub douyin_sync_script: String,
    /// 默认账号文案风格（搞笑/治愈/高级感/情绪价值）。
    pub account_style: String,
    /// 采集配额（均可配置）：每轮条数、平台比例、点赞上限。
    pub batch_size: i64,
    pub tiktok_ratio: f64,
    pub instagram_ratio: f64,
    pub max_likes: i64,
    /// 命中条件：任一命中即通过（空 = 不限制）。
    #[serde(default)]
    pub hit_conditions: Vec<String>,
    /// 排除条件：任一命中即淘汰（空 = 不限制）。
    #[serde(default)]
    pub exclude_conditions: Vec<String>,
    /// 每个条件的判定依据（条件名 -> 画面/音频/不限），由 LLM 自动分类或用户手工指定。
    #[serde(default)]
    pub condition_basis: HashMap<String, ConditionBasis>,
    /// 视觉模型提供方：ollama（本地）/ openai（OpenAI 兼容云端，如 Kimi）。
    #[serde(default = "default_ai_provider")]
    pub vision_provider: String,
    /// 文本 LLM 提供方：ollama / openai（文案生成跟随此开关）。
    #[serde(default = "default_ai_provider")]
    pub llm_provider: String,
    /// OpenAI 兼容云端配置（vision 与 llm 共用 base_url 与 key）。
    #[serde(default = "default_openai_base_url")]
    pub openai_base_url: String,
    #[serde(default)]
    pub openai_api_key: String,
    #[serde(default = "default_openai_model")]
    pub openai_vision_model: String,
    #[serde(default = "default_openai_model")]
    pub openai_llm_model: String,
    /// 发布时自动设置「自主声明 → 内容为转载信息 → 取材站外」（默认开启；
    /// 关闭后发布 sidecar 跳过自主声明步骤）。
    #[serde(default = "default_true")]
    pub publish_declaration: bool,
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
            quality_min_keep: 60,
            node_bin: "node".to_string(),
            publisher_script: "scripts/douyin-publisher/publish.js".to_string(),
            publisher_browser: "chrome".to_string(),
            collector_script: "scripts/collector/collect.js".to_string(),
            ytdlp_path: "yt-dlp".to_string(),
            douyin_sync_script: default_douyin_sync_script(),
            account_style: "治愈".to_string(),
            batch_size: 20,
            tiktok_ratio: 0.3,
            instagram_ratio: 0.7,
            max_likes: 1_000_000,
            hit_conditions: Vec::new(),
            exclude_conditions: Vec::new(),
            condition_basis: HashMap::new(),
            vision_provider: default_ai_provider(),
            llm_provider: default_ai_provider(),
            openai_base_url: default_openai_base_url(),
            openai_api_key: String::new(),
            openai_vision_model: default_openai_model(),
            openai_llm_model: default_openai_model(),
            publish_declaration: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_douyin_sync_script() -> String {
    "scripts/douyin-sync/sync.js".to_string()
}

fn default_ai_provider() -> String {
    "ollama".to_string()
}

fn default_openai_base_url() -> String {
    "https://api.kimi.com/coding/v1".to_string()
}

fn default_openai_model() -> String {
    "k3-256k".to_string()
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

    /// 自动巡检开关（独立 key，与 AppSettings JSON 分离——顶栏开关频繁读写，
    /// 不需要触碰完整设置行）。默认关。
    pub fn get_auto_sync_moderate(&self) -> Result<bool, DomainError> {
        let raw: Option<String> = self
            .db
            .conn()
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'auto_sync_moderate'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw.as_deref() == Some("1"))
    }

    pub fn set_auto_sync_moderate(&self, enabled: bool) -> Result<(), DomainError> {
        self.db.conn().execute(
            "INSERT INTO app_settings (key, value) VALUES ('auto_sync_moderate', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                updated_at = datetime('now')",
            [if enabled { "1" } else { "0" }],
        )?;
        Ok(())
    }
}
