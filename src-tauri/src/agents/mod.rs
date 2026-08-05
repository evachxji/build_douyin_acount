pub mod filter;
pub mod collector;
pub mod content;
pub mod publisher;
pub mod runtime;
pub mod vision;

use std::path::PathBuf;

use crate::db::Database;
use crate::domain::error::DomainError;
use crate::domain::task::AgentTask;
use crate::media::MediaStore;

/// Agent 执行结果。
#[derive(Debug, Clone, serde::Serialize)]
pub enum AgentOutcome {
    /// 任务成功，附结构化输出。
    Succeeded(serde_json::Value),
    /// 需要人工审批。
    WaitingForHuman(serde_json::Value),
    /// 本轮无终态：任务保持 RUNNING，worker 下一轮继续轮询（用于等待外部变化）。
    Continue,
}

/// Agent 执行上下文：提供数据库与媒体存储访问。
pub struct AgentContext<'a> {
    pub db: &'a Database,
    pub store: MediaStore,
}

impl AgentContext<'_> {
    /// 单个资产的中间产物目录（抽帧/音频），按资产 id 隔离。
    pub fn work_dir(&self, asset_id: i64) -> PathBuf {
        self.store.frames_dir().join(asset_id.to_string())
    }
}

/// 所有 Agent 的统一契约。
pub trait Agent: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(&self, ctx: &AgentContext, task: AgentTask) -> Result<AgentOutcome, DomainError>;
}
