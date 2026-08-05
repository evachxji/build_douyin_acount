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
        seed_account_styles(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 获取连接锁（调用方需尽快释放）。
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("db lock poisoned")
    }
}

/// 内置账号风格（搞笑/治愈/高级感/情绪价值），缺失时补齐。
fn seed_account_styles(conn: &Connection) -> Result<(), DomainError> {
    const STYLES: &[(&str, &str, &str)] = &[
        ("搞笑", "轻松幽默、爱玩梗，适合娱乐向账号", "用轻松幽默的口吻，加入网络热梗和夸张语气，让人会心一笑"),
        ("治愈", "温暖细腻、情绪舒缓，适合陪伴型账号", "语气温柔治愈，多用生活化细节和暖心词句，给人陪伴感"),
        ("高级感", "克制、有质感、文案极简，适合品牌/审美向账号", "文案简洁克制、有质感，少用感叹号，突出画面与氛围"),
        ("情绪价值", "共情、金句、强代入感，适合情绪向账号", "一句话戳中情绪，用共情和金句制造代入感，引发共鸣"),
    ];
    for (name, description, tone) in STYLES {
        conn.execute(
            "INSERT OR IGNORE INTO account_styles (name, description, tone_prompt) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, description, tone],
        )?;
    }
    Ok(())
}
