use rusqlite::{params, OptionalExtension, Row};

use crate::db::Database;
use crate::domain::content::{AccountStyle, ContentCopy, NewContentCopy};
use crate::domain::error::DomainError;

/// account_styles 表访问层。
pub struct StylesRepo<'a> {
    db: &'a Database,
}

impl<'a> StylesRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn list(&self) -> Result<Vec<AccountStyle>, DomainError> {
        let conn = self.db.conn();
        let mut stmt =
            conn.prepare("SELECT id, name, description, tone_prompt FROM account_styles ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(AccountStyle {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                tone_prompt: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_by_name(&self, name: &str) -> Result<Option<AccountStyle>, DomainError> {
        let row = self
            .db
            .conn()
            .query_row(
                "SELECT id, name, description, tone_prompt FROM account_styles WHERE name = ?1",
                params![name],
                |r| {
                    Ok(AccountStyle {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        description: r.get(2)?,
                        tone_prompt: r.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }
}

/// content_copies 表访问层。
pub struct ContentRepo<'a> {
    db: &'a Database,
}

fn row_to_copy(row: &Row<'_>) -> rusqlite::Result<ContentCopy> {
    let hashtags: String = row.get(5)?;
    Ok(ContentCopy {
        id: row.get(0)?,
        asset_id: row.get(1)?,
        style_id: row.get(2)?,
        title: row.get(3)?,
        body: row.get(4)?,
        hashtags: serde_json::from_str(&hashtags).unwrap_or_default(),
        created_at: row.get(6)?,
    })
}

impl<'a> ContentRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// 按 asset_id upsert（每个素材最多一份风格化文案）。
    pub fn upsert(&self, input: NewContentCopy) -> Result<i64, DomainError> {
        let hashtags = serde_json::to_string(&input.hashtags)?;
        self.db.conn().execute(
            "INSERT INTO content_copies (asset_id, style_id, title, body, hashtags_json)
             VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(asset_id) DO UPDATE SET
               style_id = excluded.style_id,
               title = excluded.title,
               body = excluded.body,
               hashtags_json = excluded.hashtags_json",
            params![
                input.asset_id,
                input.style_id,
                input.title,
                input.body,
                hashtags
            ],
        )?;
        Ok(self.get_by_asset(input.asset_id)?.expect("copy exists").id)
    }

    pub fn get_by_asset(&self, asset_id: i64) -> Result<Option<ContentCopy>, DomainError> {
        let row = self
            .db
            .conn()
            .query_row(
                "SELECT * FROM content_copies WHERE asset_id = ?1",
                params![asset_id],
                row_to_copy,
            )
            .optional()?;
        Ok(row)
    }
}
