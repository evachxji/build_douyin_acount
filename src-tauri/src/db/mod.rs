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
        ensure_analysis_conditions_column(&conn)?;
        ensure_eliminate_reason_column(&conn)?;
        ensure_music_info_column(&conn)?;
        ensure_douyin_posts_violation_columns(&conn)?;
        ensure_music_covers_url_column(&conn)?;
        reset_music_covers_not_found_once(&conn)?;
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

/// 旧库升级：analysis_results 缺 condition_results_json 列时补上（幂等，每次启动安全）。
fn ensure_analysis_conditions_column(conn: &Connection) -> Result<(), DomainError> {
    let mut stmt = conn.prepare("PRAGMA table_info(analysis_results)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<_, _>>()?;
    if !cols.iter().any(|c| c == "condition_results_json") {
        conn.execute_batch(
            "ALTER TABLE analysis_results ADD COLUMN condition_results_json TEXT NOT NULL DEFAULT '[]'",
        )?;
    }
    Ok(())
}

/// 旧库升级：video_assets 缺 eliminate_reason 列时补上（幂等，每次启动安全）。
fn ensure_eliminate_reason_column(conn: &Connection) -> Result<(), DomainError> {
    let mut stmt = conn.prepare("PRAGMA table_info(video_assets)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<_, _>>()?;
    if !cols.iter().any(|c| c == "eliminate_reason") {
        conn.execute_batch("ALTER TABLE video_assets ADD COLUMN eliminate_reason TEXT")?;
    }
    Ok(())
}

/// 旧库升级：video_assets 缺 music_info_json 列时补上（幂等，每次启动安全）。
fn ensure_music_info_column(conn: &Connection) -> Result<(), DomainError> {
    let mut stmt = conn.prepare("PRAGMA table_info(video_assets)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<_, _>>()?;
    if !cols.iter().any(|c| c == "music_info_json") {
        conn.execute_batch("ALTER TABLE video_assets ADD COLUMN music_info_json TEXT")?;
    }
    Ok(())
}

/// 旧库升级：douyin_posts 缺违规信息列时补上（幂等，每次启动安全）。
fn ensure_douyin_posts_violation_columns(conn: &Connection) -> Result<(), DomainError> {
    let mut stmt = conn.prepare("PRAGMA table_info(douyin_posts)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<_, _>>()?;
    for (name, ddl) in [
        ("violation_status", "ALTER TABLE douyin_posts ADD COLUMN violation_status INTEGER"),
        ("violation_text", "ALTER TABLE douyin_posts ADD COLUMN violation_text TEXT"),
        ("violation_brief", "ALTER TABLE douyin_posts ADD COLUMN violation_brief TEXT"),
        ("violation_url", "ALTER TABLE douyin_posts ADD COLUMN violation_url TEXT"),
    ] {
        if !cols.iter().any(|c| c == name) {
            conn.execute_batch(ddl)?;
        }
    }
    Ok(())
}

/// 旧库升级：music_covers 缺 cover_url 列时补上（幂等，每次启动安全）。
fn ensure_music_covers_url_column(conn: &Connection) -> Result<(), DomainError> {
    let mut stmt = conn.prepare("PRAGMA table_info(music_covers)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<Result<_, _>>()?;
    if !cols.is_empty() && !cols.iter().any(|c| c == "cover_url") {
        conn.execute_batch("ALTER TABLE music_covers ADD COLUMN cover_url TEXT")?;
    }
    Ok(())
}

/// 一次性升级：封面匹配加了精确源（剪映缓存）与 QQ 音乐回退后，旧的 NOT_FOUND
/// 记录全部作废重来（用 app_settings 键保证只清一次）。
fn reset_music_covers_not_found_once(conn: &Connection) -> Result<(), DomainError> {
    let done: bool = conn
        .query_row(
            "SELECT 1 FROM app_settings WHERE key = 'music_covers_reset_v2'",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if !done {
        conn.execute("DELETE FROM music_covers WHERE status = 'NOT_FOUND'", [])?;
        conn.execute(
            "INSERT OR REPLACE INTO app_settings (key, value, updated_at)
             VALUES ('music_covers_reset_v2', '1', datetime('now'))",
            [],
        )?;
    }
    Ok(())
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
