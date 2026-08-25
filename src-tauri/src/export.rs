//! 抖音作品数据导出：把 douyin_posts 快照写成 Excel 清单（含封面缩略图）。
//!
//! 只导出已发布 + 定时待发布的作品（用户需求口径），字段尽量齐全，
//! 封面以缩略图嵌入单元格（best-effort，失败不阻塞整体导出）。

use std::io::Cursor;
use std::path::Path;

use rust_xlsxwriter::{Format, FormatAlign, Workbook};

use crate::domain::douyin::DouyinPost;
use crate::domain::error::DomainError;

/// 导出统计（供任务 payload 回显给前端）。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ExportStats {
    pub rows: usize,
    pub images: usize,
    pub file_path: String,
}

/// 状态英文 -> 中文（导出列可读性）。
fn status_label(s: &str) -> &'static str {
    match s {
        "published" => "已发布",
        "scheduled" => "定时待发布",
        "reviewing" => "审核中",
        "private" => "私密",
        "prohibited" => "受限",
        "deleted" => "已删除",
        _other => "未知",
    }
}

/// epoch 秒 -> "YYYY-MM-DD HH:MM:SS"（本地时区）。0 视为空。
fn fmt_ts(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    let dt = chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.with_timezone(&chrono::Local))
        .unwrap_or_else(chrono::Local::now);
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// 把封面文件缩略成 PNG 字节（宽 136，保持比例），供嵌入单元格。
/// 解码/编码失败返回 None（best-effort，不阻塞导出）。
fn cover_thumbnail(path: &Path) -> Option<Vec<u8>> {
    let img = image::open(path).ok()?;
    let thumb = img.thumbnail(136, 9000);
    let mut buf = Vec::new();
    thumb.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png).ok()?;
    Some(buf)
}

/// 生成抖音作品 Excel 清单。
/// `posts` 应为已发布 + 定时待发布的作品（调用方已过滤）。
pub fn write_douyin_excel(posts: &[DouyinPost], dest: &Path) -> Result<ExportStats, DomainError> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("抖音作品清单")?;
    let _ = ws.set_freeze_panes(1, 0);

    let header = Format::new()
        .set_bold()
        .set_align(FormatAlign::Center)
        .set_background_color("#DDEBF7")
        .set_border(rust_xlsxwriter::FormatBorder::Thin);
    let center = Format::new().set_align(FormatAlign::Center);

    let headers = [
        "序号",
        "作品ID",
        "标题",
        "状态",
        "封面",
        "封面链接",
        "封面本地路径",
        "播放量",
        "点赞数",
        "评论数",
        "分享数",
        "完播率(%)",
        "2秒跳出率(%)",
        "违规状态",
        "违规说明",
        "违规原因",
        "违规链接",
        "创建时间",
        "发布时间",
        "同步时间",
    ];
    for (i, h) in headers.iter().enumerate() {
        ws.write_string_with_format(0, i as u16, *h, &header)?;
    }

    let widths: [(u16, f64); 20] = [
        (0, 6.0), (1, 22.0), (2, 46.0), (3, 12.0), (4, 110.0),
        (5, 42.0), (6, 44.0), (7, 10.0), (8, 10.0), (9, 10.0),
        (10, 10.0), (11, 10.0), (12, 12.0), (13, 10.0), (14, 28.0),
        (15, 18.0), (16, 34.0), (17, 20.0), (18, 20.0), (19, 20.0),
    ];
    for (col, w) in widths {
        ws.set_column_width(col, w)?;
    }

    let mut images = 0usize;
    for (idx, post) in posts.iter().enumerate() {
        let row = (idx + 1) as u32;
        ws.write_number(row, 0, (idx + 1) as f64)?;
        ws.write_string(row, 1, &post.aweme_id)?;
        ws.write_string(row, 2, post.title.as_deref().unwrap_or(""))?;
        ws.write_string_with_format(row, 3, status_label(&post.status), &center)?;
        // 封面缩略图嵌入（best-effort）。
        if let Some(path) = post.cover_path.as_deref() {
            if let Some(png) = cover_thumbnail(Path::new(path)) {
                // 行高按缩略图高度估算（像素 * 0.75 = 磅），避免与下一行重叠。
                if let Ok(img) = image::load_from_memory(&png) {
                    let h = img.height() as f64 * 0.78 + 4.0;
                    ws.set_row_height(row, h)?;
                }
                let image = rust_xlsxwriter::Image::new_from_buffer(&png)?;
                ws.insert_image(row, 4, &image)?;
                images += 1;
            }
        }
        ws.write_string(row, 5, post.cover_url.as_deref().unwrap_or(""))?;
        ws.write_string(row, 6, post.cover_path.as_deref().unwrap_or(""))?;
        ws.write_number(row, 7, post.play_count as f64)?;
        ws.write_number(row, 8, post.digg_count as f64)?;
        ws.write_number(row, 9, post.comment_count as f64)?;
        ws.write_number(row, 10, post.share_count as f64)?;
        ws.write_number(row, 11, post.completion_rate.unwrap_or(0.0))?;
        ws.write_number(row, 12, post.bounce_rate_2s.unwrap_or(0.0))?;
        ws.write_number(row, 13, post.violation_status.unwrap_or(0) as f64)?;
        ws.write_string(row, 14, post.violation_text.as_deref().unwrap_or(""))?;
        ws.write_string(row, 15, post.violation_brief.as_deref().unwrap_or(""))?;
        ws.write_string(row, 16, post.violation_url.as_deref().unwrap_or(""))?;
        ws.write_string(row, 17, fmt_ts(post.create_time))?;
        ws.write_string(row, 18, fmt_ts(post.public_time))?;
        ws.write_string(row, 19, post.synced_at.clone())?;
    }

    wb.save(dest)?;
    Ok(ExportStats {
        rows: posts.len(),
        images,
        file_path: dest.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::douyin::DouyinPost;

    fn sample_post(id: i64, aweme: &str, status: &str, cover: Option<String>) -> DouyinPost {
        DouyinPost {
            id,
            aweme_id: aweme.to_string(),
            title: Some("测试视频".to_string()),
            cover_path: cover,
            cover_url: Some("https://example.com/c.jpg".to_string()),
            status: status.to_string(),
            play_count: 1234,
            digg_count: 567,
            comment_count: 89,
            share_count: 12,
            completion_rate: Some(33.5),
            bounce_rate_2s: Some(12.3),
            violation_status: None,
            violation_text: None,
            violation_brief: None,
            violation_url: None,
            create_time: 1_750_000_000,
            public_time: 1_758_086_401,
            synced_at: "2026-08-25 10:00:00".to_string(),
            updated_at: "2026-08-25 10:00:00".to_string(),
        }
    }

    #[test]
    fn writes_valid_excel_with_embedded_cover() {
        let dir = tempfile::tempdir().unwrap();
        // 真实 PNG 封面：验证缩略图能被解码并嵌入。
        let cover = dir.path().join("cover.png");
        let img = image::RgbImage::from_pixel(320, 180, image::Rgb([200, 30, 30]));
        img.save(&cover).unwrap();

        let posts = vec![
            sample_post(
                1,
                "7001",
                "published",
                Some(cover.to_string_lossy().into_owned()),
            ),
            sample_post(2, "7002", "scheduled", None),
        ];
        let dest = dir.path().join("out.xlsx");
        let stats = write_douyin_excel(&posts, &dest).unwrap();
        assert_eq!(stats.rows, 2);
        assert_eq!(stats.images, 1);
        assert!(dest.exists());
        // xlsx 本质是 zip 容器。
        let bytes = std::fs::read(&dest).unwrap();
        assert!(bytes.starts_with(b"PK"));
    }

    #[test]
    fn skips_missing_cover_without_failing() {
        let dir = tempfile::tempdir().unwrap();
        // 封面路径指向不存在的文件：应跳过图片但导出不失败。
        let posts = vec![sample_post(
            1,
            "7001",
            "published",
            Some(dir.path().join("missing.png").to_string_lossy().into_owned()),
        )];
        let dest = dir.path().join("out.xlsx");
        let stats = write_douyin_excel(&posts, &dest).unwrap();
        assert_eq!(stats.rows, 1);
        assert_eq!(stats.images, 0);
        assert!(dest.exists());
    }
}
