use serde::{Deserialize, Serialize};

use super::error::DomainError;

/// Agent 任务生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskStatus {
    Pending,
    Running,
    WaitingForHuman,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskStatus {
    /// 是否允许 from -> to 的合法状态迁移。
    pub fn can_transition_to(&self, to: &TaskStatus) -> bool {
        use TaskStatus::*;
        match (self, to) {
            (Pending, Running) => true,
            (Running, Succeeded | Failed | WaitingForHuman) => true,
            (WaitingForHuman, Running | Cancelled) => true,
            (Failed, Pending) => true, // 重试
            _ => false,
        }
    }

    /// 校验并返回迁移结果。
    pub fn check_transition(&self, to: &TaskStatus) -> Result<(), DomainError> {
        if self.can_transition_to(to) {
            Ok(())
        } else {
            Err(DomainError::IllegalTransition {
                from: self.to_string(),
                to: to.to_string(),
            })
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TaskStatus::Pending => "PENDING",
            TaskStatus::Running => "RUNNING",
            TaskStatus::WaitingForHuman => "WAITING_FOR_HUMAN",
            TaskStatus::Succeeded => "SUCCEEDED",
            TaskStatus::Failed => "FAILED",
            TaskStatus::Cancelled => "CANCELLED",
        };
        write!(f, "{s}")
    }
}

impl std::str::FromStr for TaskStatus {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "PENDING" => Ok(TaskStatus::Pending),
            "RUNNING" => Ok(TaskStatus::Running),
            "WAITING_FOR_HUMAN" => Ok(TaskStatus::WaitingForHuman),
            "SUCCEEDED" => Ok(TaskStatus::Succeeded),
            "FAILED" => Ok(TaskStatus::Failed),
            "CANCELLED" => Ok(TaskStatus::Cancelled),
            other => Err(DomainError::InvalidData(format!("unknown task status: {other}"))),
        }
    }
}

/// Agent 任务记录。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentTask {
    pub id: i64,
    pub agent_name: String,
    pub task_type: String,
    pub payload: serde_json::Value,
    pub status: TaskStatus,
    pub attempts: i64,
    pub max_attempts: i64,
    pub next_retry_at: Option<String>,
    pub human_required: bool,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 任务日志条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLog {
    pub id: i64,
    pub task_id: i64,
    pub level: String,
    pub message: String,
    pub created_at: String,
}

/// 指数退避：30s * 2^(failure_count)，上限 1 小时。
pub fn retry_delay_secs(failure_count: i64) -> i64 {
    let base = 30;
    let exp = failure_count.clamp(0, 7);
    let secs = base * 2_i64.pow(exp as u32);
    secs.min(3600)
}
