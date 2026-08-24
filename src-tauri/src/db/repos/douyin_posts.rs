use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};

use crate::db::Database;
use crate::domain::douyin::{DouyinPost, NewDouyinPost};
use crate::domain::error::DomainError;

/// douyin_posts 表访问层。
pub struct DouyinPostsRepo<'a> {
    db: &'a Database,
}

const POST_COLS: &str = "id, aweme_id, title, cover_path, cover_url, status, play_count,
       digg_count, comment_count, share_count, completion_rate, bounce_rate_2s,
       violation_status, violation_text, violation_brief, violation_url,
       create_time, public_time, synced_at, updated_at";

fn row_to_post(row: &Row<'_>) -> rusqlite::Result<DouyinPost> {
    Ok(DouyinPost {
        id: row.get(0)?,
        aweme_id: row.get(1)?,
        title: row.get(2)?,
        cover_path: row.get(3)?,
        cover_url: row.get(4)?,
        status: row.get(5)?,
        play_count: row.get(6)?,
        digg_count: row.get(7)?,
        comment_count: row.get(8)?,
        share_count: row.get(9)?,
        completion_rate: row.get(10)?,
        bounce_rate_2s: row.get(11)?,
        violation_status: row.get(12)?,
        violation_text: row.get(13)?,
        violation_brief: row.get(14)?,
        violation_url: row.get(15)?,
        create_time: row.get(16)?,
        public_time: row.get(17)?,
        synced_at: row.get(18)?,
        updated_at: row.get(19)?,
    })
}

impl<'a> DouyinPostsRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// 按 aweme_id 幂等 upsert；本轮没下到封面时保留旧封面。
    /// 违规字段语义：status/text/url 以本轮列表为准（违规消解则清掉）；
    /// brief 来自详情接口（本轮没补抓到时保留旧值），违规消解时随 status 一并清空。
    pub fn upsert(&self, p: &NewDouyinPost) -> Result<(), DomainError> {
        let conn = self.db.conn();
        conn.execute(
            "INSERT INTO douyin_posts (aweme_id, title, cover_path, cover_url, status,
                 play_count, digg_count, comment_count, share_count,
                 completion_rate, bounce_rate_2s,
                 violation_status, violation_text, violation_brief, violation_url,
                 create_time, public_time)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
             ON CONFLICT(aweme_id) DO UPDATE SET
                 title = excluded.title,
                 cover_path = COALESCE(excluded.cover_path, douyin_posts.cover_path),
                 cover_url = excluded.cover_url,
                 status = excluded.status,
                 play_count = excluded.play_count,
                 digg_count = excluded.digg_count,
                 comment_count = excluded.comment_count,
                 share_count = excluded.share_count,
                 completion_rate = COALESCE(excluded.completion_rate, douyin_posts.completion_rate),
                 bounce_rate_2s = COALESCE(excluded.bounce_rate_2s, douyin_posts.bounce_rate_2s),
                 violation_status = excluded.violation_status,
                 violation_text = excluded.violation_text,
                 violation_brief = CASE WHEN excluded.violation_status IS NULL THEN NULL
                     ELSE COALESCE(excluded.violation_brief, douyin_posts.violation_brief) END,
                 violation_url = CASE WHEN excluded.violation_status IS NULL THEN NULL
                     ELSE COALESCE(excluded.violation_url, douyin_posts.violation_url) END,
                 create_time = excluded.create_time,
                 public_time = excluded.public_time,
                 updated_at = ?18",
            params![
                p.aweme_id,
                p.title,
                p.cover_path,
                p.cover_url,
                p.status,
                p.play_count,
                p.digg_count,
                p.comment_count,
                p.share_count,
                p.completion_rate,
                p.bounce_rate_2s,
                p.violation_status,
                p.violation_text,
                p.violation_brief,
                p.violation_url,
                p.create_time,
                p.public_time,
                now(),
            ],
        )?;
        Ok(())
    }

    /// 按创建时间倒序（已发布=发布时间，定时作品=定时发布的未来时间，排在最前）。
    pub fn list(&self, limit: i64) -> Result<Vec<DouyinPost>, DomainError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {POST_COLS} FROM douyin_posts ORDER BY create_time DESC LIMIT ?1"
        ))?;
        let rows = stmt.query_map(params![limit], row_to_post)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn count(&self) -> Result<i64, DomainError> {
        let conn = self.db.conn();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM douyin_posts", [], |r| r.get(0))?;
        Ok(n)
    }

    /// 已存有违规原因（brief）的作品 aweme_id 列表——同步时传给 sidecar 跳过详情补抓。
    pub fn list_violation_brief_ids(&self) -> Result<Vec<String>, DomainError> {
        let conn = self.db.conn();
        let mut stmt =
            conn.prepare("SELECT aweme_id FROM douyin_posts WHERE violation_brief IS NOT NULL")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_by_aweme(&self, aweme_id: &str) -> Result<Option<DouyinPost>, DomainError> {
        let conn = self.db.conn();
        let row = conn
            .query_row(
                &format!("SELECT {POST_COLS} FROM douyin_posts WHERE aweme_id = ?1"),
                params![aweme_id],
                row_to_post,
            )
            .optional()?;
        Ok(row)
    }

    /// 当前仍标记违规且未私密的作品（violation_text 非空），按发布时间倒序。
    /// 已私密的作品已是「仅自己可见」目标状态，不再重复处理。
    pub fn list_violating(&self) -> Result<Vec<DouyinPost>, DomainError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {POST_COLS} FROM douyin_posts
             WHERE violation_text IS NOT NULL AND status != 'private'
             ORDER BY create_time DESC"
        ))?;
        let rows = stmt.query_map([], row_to_post)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 定时发布中的作品（最晚的在前）。
    pub fn list_scheduled(&self) -> Result<Vec<DouyinPost>, DomainError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {POST_COLS} FROM douyin_posts WHERE status = 'scheduled'
             ORDER BY create_time DESC"
        ))?;
        let rows = stmt.query_map([], row_to_post)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// 「有公开视频」的本地日期集合（发布日历口径：私密作品不参与标记、不占位）。
    /// 日期取 postTime（public_time > 0 优先，否则 create_time）的本地日，
    /// 与前端 calendar.ts 的 groupPostsByDay/dayMarks 口径一致。
    pub fn list_occupied_days(
        &self,
    ) -> Result<std::collections::HashSet<chrono::NaiveDate>, DomainError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT date(
                 CASE WHEN public_time > 0 THEN public_time ELSE create_time END,
                 'unixepoch', 'localtime')
             FROM douyin_posts WHERE status != 'private'",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|s| chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
            .collect())
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}
