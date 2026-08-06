use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::watch;

use crate::db::Database;
use crate::db::repos::tasks::TasksRepo;
use crate::media::MediaStore;

use super::{Agent, AgentOutcome};

/// 一次 `run_once` 的处理结果。
#[derive(Debug)]
pub struct RunResult {
    processed: bool,
}

impl RunResult {
    /// 是否实际处理了任务。
    pub fn is_processed(&self) -> bool {
        self.processed
    }
}

/// Agent Runtime：SQLite 持久化队列 + 状态机 + 重试 + 日志。
pub struct AgentRuntime<'a> {
    db: &'a Database,
    store: MediaStore,
    agents: HashMap<&'static str, Box<dyn Agent>>,
}

impl<'a> AgentRuntime<'a> {
    pub fn new(db: &'a Database, store: MediaStore) -> Self {
        Self {
            db,
            store,
            agents: HashMap::new(),
        }
    }

    pub fn register(&mut self, agent: Box<dyn Agent>) {
        self.agents.insert(agent.name(), agent);
    }

    pub fn agent_names(&self) -> Vec<&'static str> {
        self.agents.keys().copied().collect()
    }

    /// 领取并执行一个任务（同一 Agent 串行）。
    pub fn run_once(&self, agent_name: &str) -> Result<RunResult, crate::domain::error::DomainError> {
        let Some(agent) = self.agents.get(agent_name) else {
            return Ok(RunResult { processed: false });
        };
        let tasks = TasksRepo::new(self.db);
        let Some(task) = tasks.claim_next(agent_name)? else {
            return Ok(RunResult { processed: false });
        };
        tasks.add_log(task.id, "INFO", &format!("task {} started", task.task_type))?;
        let outcome = agent.run(&crate::agents::AgentContext {
            db: self.db,
            store: self.store.clone(),
        }, task.clone());
        match outcome {
            Ok(AgentOutcome::Succeeded(output)) => {
                tasks.add_log(task.id, "INFO", &format!("task succeeded: {output}"))?;
                // 把执行结果合并进任务 payload，供前端任务面板展示执行结果。
                let _ = tasks.update_payload(task.id, output);
                tasks.succeed(task.id)?;
            }
            Ok(AgentOutcome::WaitingForHuman(ctx_data)) => {
                tasks.add_log(task.id, "WARN", &format!("task waiting for human: {ctx_data}"))?;
                tasks.wait_for_human_with_context(task.id, ctx_data)?;
            }
            Ok(AgentOutcome::Continue) => {
                // 无终态：任务保持 RUNNING，下一轮 run_once 重新领取继续轮询。
            }
            Err(e) => {
                tasks.add_log(task.id, "ERROR", &e.to_string())?;
                tasks.fail(task.id, &e.to_string())?;
            }
        }
        Ok(RunResult { processed: true })
    }

    /// 轮询循环：按给定间隔执行各 Agent，直到收到关闭信号。
    pub async fn run_loop(
        &self,
        agent_names: &[&'static str],
        shutdown: watch::Receiver<bool>,
        interval: Duration,
    ) {
        loop {
            if *shutdown.borrow() {
                break;
            }
            for name in agent_names {
                let _ = self.run_once(name);
            }
            tokio::time::sleep(interval).await;
        }
    }
}
