use chrono::Utc;
use rusqlite::{params, params_from_iter, OptionalExtension, Row};

use crate::db::Database;
use crate::domain::asset::{AssetStatus, NewAsset, VideoAsset};
use crate::domain::error::DomainError;

/// 资产列表筛选条件。
#[derive(Debug, Default, Clone)]
pub struct AssetFilter {
    pub status: Option<AssetStatus>,
    pub platform: Option<String>,
    pub favorite: Option<bool>,
    pub category: Option<String>,
    pub search: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

/// video_assets 表访问层。
pub struct AssetsRepo<'a> {
    db: &'a Database,
}

const ASSET_COLS: &str = "va.id, va.dedup_key, va.file_path, va.cover_path, va.source_platform,
    va.source_url, va.author, va.title, va.description, va.published_at, va.tags_json,
    va.likes, va.comments, va.plays, va.duration_ms, va.status, va.is_favorite,
    ar.category AS category, ar.quality_score AS quality_score,
    va.created_at, va.updated_at";

fn row_to_asset(row: &Row<'_>) -> rusqlite::Result<VideoAsset> {
    let tags: String = row.get(10)?;
    let status: String = row.get(15)?;
    let favorite: i64 = row.get(16)?;
    Ok(VideoAsset {
        id: row.get(0)?,
        dedup_key: row.get(1)?,
        file_path: row.get(2)?,
        cover_path: row.get(3)?,
        source_platform: row.get(4)?,
        source_url: row.get(5)?,
        author: row.get(6)?,
        title: row.get(7)?,
        description: row.get(8)?,
        published_at: row.get(9)?,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        likes: row.get(11)?,
        comments: row.get(12)?,
        plays: row.get(13)?,
        duration_ms: row.get(14)?,
        status: status
            .parse()
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(15, rusqlite::types::Type::Text, Box::new(e)))?,
        is_favorite: favorite != 0,
        category: row.get(17)?,
        quality_score: row.get(18)?,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
    })
}

impl<'a> AssetsRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// 插入新资产；dedup_key 冲突时返回 `Duplicate`。
    pub fn insert(&self, input: NewAsset) -> Result<i64, DomainError> {
        let conn = self.db.conn();
        let tags = serde_json::to_string(&input.tags)?;
        let res = conn.execute(
            "INSERT INTO video_assets
                (dedup_key, file_path, cover_path, source_platform, source_url, author,
                 title, description, published_at, tags_json, likes, comments, plays, duration_ms)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![
                input.dedup_key,
                input.file_path,
                input.cover_path,
                input.source_platform,
                input.source_url,
                input.author,
                input.title,
                input.description,
                input.published_at,
                tags,
                input.likes,
                input.comments,
                input.plays,
                input.duration_ms,
            ],
        );
        match res {
            Ok(_) => Ok(conn.last_insert_rowid()),
            Err(rusqlite::Error::SqliteFailure(e, Some(msg))) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
                Err(DomainError::Duplicate(msg))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn get(&self, id: i64) -> Result<Option<VideoAsset>, DomainError> {
        let sql = format!(
            "SELECT {ASSET_COLS} FROM video_assets va
             LEFT JOIN analysis_results ar ON ar.asset_id = va.id
             WHERE va.id = ?1"
        );
        let asset = self
            .db
            .conn()
            .query_row(&sql, params![id], row_to_asset)
            .optional()?;
        Ok(asset)
    }

    pub fn find_by_dedup(&self, dedup_key: &str) -> Result<Option<VideoAsset>, DomainError> {
        let sql = format!(
            "SELECT {ASSET_COLS} FROM video_assets va
             LEFT JOIN analysis_results ar ON ar.asset_id = va.id
             WHERE va.dedup_key = ?1"
        );
        let asset = self
            .db
            .conn()
            .query_row(&sql, params![dedup_key], row_to_asset)
            .optional()?;
        Ok(asset)
    }

    /// 按条件查询资产列表（category 为联表过滤）。
    pub fn list(&self, filter: AssetFilter) -> Result<Vec<VideoAsset>, DomainError> {
        let mut conditions: Vec<String> = Vec::new();
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(s) = filter.status {
            conditions.push("va.status = ?".to_string());
            values.push(Box::new(s.to_string()));
        }
        if let Some(p) = filter.platform {
            conditions.push("va.source_platform = ?".to_string());
            values.push(Box::new(p));
        }
        if let Some(f) = filter.favorite {
            conditions.push("va.is_favorite = ?".to_string());
            values.push(Box::new(f as i64));
        }
        if let Some(c) = filter.category {
            conditions.push("ar.category = ?".to_string());
            values.push(Box::new(c));
        }
        if let Some(q) = filter.search {
            conditions.push("(va.title LIKE ? OR va.author LIKE ? OR ar.summary LIKE ?)".to_string());
            let like = format!("%{q}%");
            values.push(Box::new(like.clone()));
            values.push(Box::new(like.clone()));
            values.push(Box::new(like));
        }
        let limit = if filter.limit > 0 { filter.limit } else { 100 };
        let offset = filter.offset.max(0);
        let where_sql = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };
        values.push(Box::new(limit));
        values.push(Box::new(offset));
        let sql = format!(
            "SELECT {ASSET_COLS} FROM video_assets va
             LEFT JOIN analysis_results ar ON ar.asset_id = va.id
             {where_sql}
             ORDER BY va.created_at DESC LIMIT ? OFFSET ?"
        );
        let conn = self.db.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values.iter()), row_to_asset)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 状态迁移（内部校验合法性）。
    pub fn update_status(&self, id: i64, to: AssetStatus) -> Result<(), DomainError> {
        let conn = self.db.conn();
        let current: Option<String> = conn
            .query_row("SELECT status FROM video_assets WHERE id = ?1", params![id], |r| r.get(0))
            .optional()?;
        let Some(cur) = current else {
            return Err(DomainError::AssetNotFound(id));
        };
        let from: AssetStatus = cur
            .parse()
            .map_err(|_| DomainError::InvalidData(format!("unknown status: {cur}")))?;
        from.check_transition(&to)?;
        conn.execute(
            "UPDATE video_assets SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![to.to_string(), now(), id],
        )?;
        Ok(())
    }

    pub fn set_favorite(&self, id: i64, favorite: bool) -> Result<(), DomainError> {
        let n = self.db.conn().execute(
            "UPDATE video_assets SET is_favorite = ?1, updated_at = ?2 WHERE id = ?3",
            params![favorite as i64, now(), id],
        )?;
        if n == 0 {
            return Err(DomainError::AssetNotFound(id));
        }
        Ok(())
    }

    /// 更新素材互动数据（采集器入库后回填点赞/评论/播放数）。
    pub fn update_stats(
        &self,
        id: i64,
        likes: i64,
        comments: i64,
        plays: i64,
    ) -> Result<(), DomainError> {
        let n = self.db.conn().execute(
            "UPDATE video_assets SET likes = ?1, comments = ?2, plays = ?3,
             updated_at = ?4 WHERE id = ?5",
            params![likes, comments, plays, now(), id],
        )?;
        if n == 0 {
            return Err(DomainError::AssetNotFound(id));
        }
        Ok(())
    }

    /// 删除资产（分析结果、草稿、推荐等通过外键级联删除；文件删除由媒体服务负责）。
    pub fn delete(&self, id: i64) -> Result<(), DomainError> {
        let n = self.db.conn().execute("DELETE FROM video_assets WHERE id = ?1", params![id])?;
        if n == 0 {
            return Err(DomainError::AssetNotFound(id));
        }
        Ok(())
    }

    pub fn count_by_status(&self, status: AssetStatus) -> Result<i64, DomainError> {
        let n = self.db.conn().query_row(
            "SELECT COUNT(*) FROM video_assets WHERE status = ?1",
            params![status.to_string()],
            |r| r.get(0),
        )?;
        Ok(n)
    }

    /// 按分析分类统计资产数（未分析归入"未分类"），供左侧分类树使用。
    pub fn category_counts(&self) -> Result<Vec<(String, i64)>, DomainError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT COALESCE(ar.category, '未分类') AS category, COUNT(*) AS c
             FROM video_assets va
             LEFT JOIN analysis_results ar ON ar.asset_id = va.id
             GROUP BY category
             ORDER BY c DESC, category ASC",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 按状态统计资产数，供左侧状态分组与顶栏使用。
    pub fn status_counts(&self) -> Result<Vec<(String, i64)>, DomainError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT status, COUNT(*) FROM video_assets GROUP BY status ORDER BY status ASC",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}
