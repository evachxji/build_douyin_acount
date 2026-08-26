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
    /// 每次 upsert 都用 datetime('now') 刷新 synced_at（与列 DEFAULT 同格式，
    /// 前端 lastSynced 按字符串取 max 依赖全表格式统一）。
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
                 synced_at = datetime('now'),
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

    /// 导出清单数据源：仅已发布 + 定时待发布作品，按创建时间倒序。
    pub fn list_for_export(&self) -> Result<Vec<DouyinPost>, DomainError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {POST_COLS} FROM douyin_posts
             WHERE status IN ('published','scheduled')
             ORDER BY create_time DESC"
        ))?;
        let rows = stmt.query_map([], row_to_post)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
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

    /// 清理发布占位行：真实作品入库后，删除对应的 local-draft-* 占位行
    /// （publisher 发布登记时钩子未捕获到真实 aweme_id 的回退产物）。
    /// 匹配条件（任一命中）：
    ///   A. create_time 精确相等——定时作品的占位与真实行同为定时发布时间，精确到分钟；
    ///   B. 发布日期相同且占位标题是真实标题前缀——占位 title 是 draft/页面标题行，
    ///      真实行 title 是抖音完整文案（标题+正文+话题），完整相等永远匹配不上（旧条件
    ///      「标题相同」的 bug：占位 title=「风与浪之间…🍃」，真实 title=完整文案，幽灵占位永存）。
    /// 仅匹配 local- 前缀，真实行永不误删；占位行误删的代价只是下轮 sync 拉到真实行前暂时不显示。
    pub fn delete_local_placeholders(
        &self,
        title: &str,
        create_time: i64,
    ) -> Result<usize, DomainError> {
        let conn = self.db.conn();
        // instr(?1, title) = 1：占位标题是真实标题前缀（不用 GLOB——标题含 * ? [ 会被当通配符）。
        // length(title) > 0：空串/NULL 标题不许走前缀匹配（否则同日全清）。
        let n = conn.execute(
            "DELETE FROM douyin_posts
             WHERE aweme_id LIKE 'local-draft-%'
               AND (
                 create_time = ?2
                 OR (date(create_time, 'unixepoch', 'localtime') = date(?2, 'unixepoch', 'localtime')
                     AND length(title) > 0 AND instr(?1, title) = 1)
               )",
            params![title, create_time],
        )?;
        Ok(n)
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

    /// 每个「有公开视频」的本地日期及其视频的时刻（当天 0 点起的分钟数，本地时区）。
    /// 口径：postTime（public_time > 0 优先，否则 create_time）的本地日，私密作品不参与，
    /// 与前端 calendar.ts 的 groupPostsByDay/dayMarks 口径一致。
    /// 供 pick_schedule_time 统计每天视频数与上午/下午/晚上时段占用。
    pub fn list_day_video_minutes(
        &self,
    ) -> Result<Vec<(chrono::NaiveDate, i64)>, DomainError> {
        let conn = self.db.conn();
        let mut stmt = conn.prepare(
            "SELECT date(t, 'unixepoch', 'localtime'),
                    CAST(strftime('%H', t, 'unixepoch', 'localtime') AS INTEGER) * 60 +
                    CAST(strftime('%M', t, 'unixepoch', 'localtime') AS INTEGER)
             FROM (SELECT CASE WHEN public_time > 0 THEN public_time ELSE create_time END AS t
                   FROM douyin_posts WHERE status != 'private')",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|(s, m)| {
                chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                    .ok()
                    .map(|d| (d, m))
            })
            .collect())
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}
