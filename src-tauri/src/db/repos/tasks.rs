use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};

use crate::db::Database;
use crate::domain::error::DomainError;
use crate::domain::task::{retry_delay_secs, AgentTask, TaskLog, TaskStatus};

/// agent_tasks 表访问层（Agent Runtime 的持久化队列）。
pub struct TasksRepo<'a> {
    db: &'a Database,
}

fn row_to_task(row: &Row<'_>) -> rusqlite::Result<AgentTask> {
    let payload: String = row.get(3)?;
    let status: String = row.get(4)?;
    Ok(AgentTask {
        id: row.get(0)?,
        agent_name: row.get(1)?,
        task_type: row.get(2)?,
        payload: serde_json::from_str(&payload).unwrap_or_default(),
        status: status
            .parse()
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?,
        attempts: row.get(5)?,
        max_attempts: row.get(6)?,
        next_retry_at: row.get(7)?,
        human_required: row.get::<_, i64>(8)? != 0,
        error: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

impl<'a> TasksRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn enqueue(
        &self,
        agent_name: &str,
        task_type: &str,
        payload: serde_json::Value,
        human_required: bool,
    ) -> Result<i64, DomainError> {
        let payload = serde_json::to_string(&payload)?;
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO agent_tasks (agent_name, task_type, payload_json, human_required)
             VALUES (?1,?2,?3,?4)",
            params![agent_name, task_type, payload, human_required as i64],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Option<AgentTask>, DomainError> {
        let row = self
            .db
            .conn()
            .query_row("SELECT * FROM agent_tasks WHERE id = ?1", params![id], row_to_task)
            .optional()?;
        Ok(row)
    }

    /// 任务列表；status 为 None 时返回全部（最新在前），供 GUI 任务面板轮询。
    pub fn list(&self, status: Option<TaskStatus>) -> Result<Vec<AgentTask>, DomainError> {
        let conn = self.db.conn();
        let mut stmt = match status {
            Some(_) => conn.prepare(
                "SELECT * FROM agent_tasks WHERE status = ?1 ORDER BY created_at DESC",
            )?,
            None => conn.prepare("SELECT * FROM agent_tasks ORDER BY created_at DESC")?,
        };
        let rows = match status {
            Some(s) => stmt
                .query_map(params![s.to_string()], row_to_task)?
                .collect::<Result<Vec<_>, _>>()?,
            None => stmt
                .query_map([], row_to_task)?
                .collect::<Result<Vec<_>, _>>()?,
        };
        Ok(rows)
    }

    /// 领取下一个可执行任务（同一 Agent 串行：仅 WAITING_FOR_HUMAN 阻塞；
    /// RUNNING 允许重新领取，用于人工 resume 与进程崩溃后的恢复）。
    pub fn claim_next(&self, agent_name: &str) -> Result<Option<AgentTask>, DomainError> {
        let conn = self.db.conn();
        let blocked: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM agent_tasks
                WHERE agent_name = ?1 AND status = 'WAITING_FOR_HUMAN')",
            params![agent_name],
            |r| r.get(0),
        )?;
        if blocked {
            return Ok(None);
        }
        let now_ts = now();
        // 1) 优先认领遗留的 RUNNING：人工 resume 后继续执行，或崩溃恢复。
        let running = conn
            .query_row(
                "SELECT * FROM agent_tasks
                 WHERE agent_name = ?1 AND status = 'RUNNING'
                 ORDER BY created_at ASC LIMIT 1",
                params![agent_name],
                row_to_task,
            )
            .optional()?;
        // 2) 无 RUNNING 时领取 PENDING（重试退避到期的才可领）。
        let task = if let Some(t) = running {
            Some(t)
        } else {
            conn.query_row(
                "SELECT * FROM agent_tasks
                 WHERE agent_name = ?1 AND status = 'PENDING'
                   AND (next_retry_at IS NULL OR next_retry_at <= ?2)
                 ORDER BY created_at ASC LIMIT 1",
                params![agent_name, now_ts],
                row_to_task,
            )
            .optional()?
        };
        let Some(task) = task else {
            return Ok(None);
        };
        // 仅 PENDING -> RUNNING 视为新一次尝试；重新认领 RUNNING 不累加。
        let attempts = if task.status == TaskStatus::Pending {
            task.attempts + 1
        } else {
            task.attempts
        };
        conn.execute(
            "UPDATE agent_tasks SET status = 'RUNNING', attempts = ?1,
             error = NULL, updated_at = ?2 WHERE id = ?3",
            params![attempts, now_ts, task.id],
        )?;
        Ok(Some(AgentTask {
            status: TaskStatus::Running,
            attempts,
            error: None,
            updated_at: now_ts,
            ..task
        }))
    }

    pub fn succeed(&self, id: i64) -> Result<(), DomainError> {
        self.transition(id, TaskStatus::Succeeded)
    }

    pub fn wait_for_human(&self, id: i64) -> Result<(), DomainError> {
        self.transition(id, TaskStatus::WaitingForHuman)
    }

    /// 向任务 payload 合并字段（保留执行字段 draft_id/asset_id），供前端读取阶段信息。
    /// 是否有进行中（PENDING/RUNNING）的任务：采集按钮防重，避免并发 sidecar 抢占浏览器。
    pub fn has_active(&self, agent_name: &str) -> Result<bool, DomainError> {
        let conn = self.db.conn();
        let exists: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM agent_tasks
                WHERE agent_name = ?1 AND status IN ('PENDING','RUNNING'))",
            params![agent_name],
            |r| r.get(0),
        )?;
        Ok(exists)
    }

    pub fn update_payload(&self, id: i64, patch: serde_json::Value) -> Result<(), DomainError> {
        let conn = self.db.conn();
        let current: Option<String> = conn
            .query_row(
                "SELECT payload_json FROM agent_tasks WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        let mut payload: serde_json::Value = current
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_else(|| serde_json::json!({}));
        if let (serde_json::Value::Object(map), serde_json::Value::Object(p)) =
            (&mut payload, &patch)
        {
            map.extend(p.clone());
        }
        let payload = serde_json::to_string(&payload)?;
        conn.execute(
            "UPDATE agent_tasks SET payload_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![payload, now(), id],
        )?;
        Ok(())
    }

    /// 等待人工确认，并把 Agent 返回的上下文（stage/message/path 等）合并进任务
    /// payload 持久化，供前端审批弹窗读取（不覆盖 draft_id/asset_id 等执行字段）。
    pub fn wait_for_human_with_context(
        &self,
        id: i64,
        context: serde_json::Value,
    ) -> Result<(), DomainError> {
        let conn = self.db.conn();
        let current: Option<String> = conn
            .query_row(
                "SELECT payload_json FROM agent_tasks WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        let mut payload: serde_json::Value = current
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_else(|| serde_json::json!({}));
        if let (serde_json::Value::Object(map), serde_json::Value::Object(ctx)) =
            (&mut payload, &context)
        {
            map.extend(ctx.clone());
        }
        let payload = serde_json::to_string(&payload)?;
        conn.execute(
            "UPDATE agent_tasks SET status = 'WAITING_FOR_HUMAN', payload_json = ?1,
             updated_at = ?2 WHERE id = ?3",
            params![payload, now(), id],
        )?;
        Ok(())
    }

    pub fn resume(&self, id: i64) -> Result<(), DomainError> {
        self.transition(id, TaskStatus::Running)
    }

    pub fn cancel(&self, id: i64) -> Result<(), DomainError> {
        self.transition(id, TaskStatus::Cancelled)
    }

    /// 失败处理：未超过最大次数则排入重试（指数退避），否则置 FAILED。
    pub fn fail(&self, id: i64, error: &str) -> Result<(), DomainError> {
        let task = self.get(id)?.ok_or(DomainError::InvalidData(format!("task {id} not found")))?;
        task.status.check_transition(&TaskStatus::Failed)?;
        let conn = self.db.conn();
        let next = if task.attempts >= task.max_attempts {
            "FAILED".to_string()
        } else {
            let delay = retry_delay_secs(task.attempts);
            let retry_at = chrono::DateTime::parse_from_rfc3339(&now())
                .map(|t| t + chrono::Duration::seconds(delay))
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|_| now());
            conn.execute(
                "UPDATE agent_tasks SET next_retry_at = ?1 WHERE id = ?2",
                params![retry_at, id],
            )?;
            "PENDING".to_string()
        };
        conn.execute(
            "UPDATE agent_tasks SET status = ?1, error = ?2, updated_at = ?3 WHERE id = ?4",
            params![next, error, now(), id],
        )?;
        Ok(())
    }

    pub fn add_log(&self, task_id: i64, level: &str, message: &str) -> Result<(), DomainError> {
        self.db.conn().execute(
            "INSERT INTO task_logs (task_id, level, message) VALUES (?1,?2,?3)",
            params![task_id, level, message],
        )?;
        Ok(())
    }

    pub fn logs(&self, task_id: i64) -> Result<Vec<TaskLog>, DomainError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT id, task_id, level, message, created_at FROM task_logs WHERE task_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt.query_map(params![task_id], |r| {
            Ok(TaskLog {
                id: r.get(0)?,
                task_id: r.get(1)?,
                level: r.get(2)?,
                message: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn transition(&self, id: i64, to: TaskStatus) -> Result<(), DomainError> {
        let task = self.get(id)?.ok_or(DomainError::InvalidData(format!("task {id} not found")))?;
        task.status.check_transition(&to)?;
        let conn = self.db.conn();
        conn.execute(
            "UPDATE agent_tasks SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![to.to_string(), now(), id],
        )?;
        Ok(())
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}
