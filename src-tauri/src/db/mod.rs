use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::domain::error::DomainError;

pub mod migrations;
pub mod repos;

/// SQLite 数据库封装：内部持有一个 Mutex 保护的连接（单写者模型，适用于本地单机）。
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// 打开（或创建）数据库并执行迁移。
    pub fn open(path: &Path) -> Result<Self, DomainError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(migrations::MIGRATIONS)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 获取连接锁（调用方需尽快释放）。
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("db lock poisoned")
    }
}
