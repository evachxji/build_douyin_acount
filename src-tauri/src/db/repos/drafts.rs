use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};

use crate::db::Database;
use crate::domain::draft::{DraftStatus, NewDraft, PublishDraft};
use crate::domain::error::DomainError;

/// publish_drafts 表访问层。
pub struct DraftsRepo<'a> {
    db: &'a Database,
}

fn row_to_draft(row: &Row<'_>) -> rusqlite::Result<PublishDraft> {
    let status: String = row.get(6)?;
    Ok(PublishDraft {
        id: row.get(0)?,
        asset_id: row.get(1)?,
        style_id: row.get(2)?,
        title: row.get(3)?,
        body: row.get(4)?,
        hashtags: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
        status: status
            .parse()
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e)))?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

impl<'a> DraftsRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn create(&self, input: NewDraft) -> Result<i64, DomainError> {
        let hashtags = serde_json::to_string(&input.hashtags)?;
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO publish_drafts (asset_id, style_id, title, body, hashtags_json)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                input.asset_id,
                input.style_id,
                input.title,
                input.body,
                hashtags
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get(&self, id: i64) -> Result<Option<PublishDraft>, DomainError> {
        let row = self
            .db
            .conn()
            .query_row(
                "SELECT * FROM publish_drafts WHERE id = ?1",
                params![id],
                row_to_draft,
            )
            .optional()?;
        Ok(row)
    }

    pub fn list(&self) -> Result<Vec<PublishDraft>, DomainError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare("SELECT * FROM publish_drafts ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], row_to_draft)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn update_status(&self, id: i64, to: DraftStatus) -> Result<(), DomainError> {
        let conn = self.db.conn();
        let current: Option<String> = conn
            .query_row(
                "SELECT status FROM publish_drafts WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(cur) = current else {
            return Err(DomainError::InvalidData(format!("draft {id} not found")));
        };
        let from: DraftStatus = cur
            .parse()
            .map_err(|_| DomainError::InvalidData(format!("unknown draft status: {cur}")))?;
        from.check_transition(&to)?;
        conn.execute(
            "UPDATE publish_drafts SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![to.to_string(), now(), id],
        )?;
        Ok(())
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}
