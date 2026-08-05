use rusqlite::{params, OptionalExtension, Row};

use crate::db::Database;
use crate::domain::analysis::{AnalysisResult, NewAnalysis};
use crate::domain::error::DomainError;

/// analysis_results 表访问层。
pub struct AnalysesRepo<'a> {
    db: &'a Database,
}

fn row_to_analysis(row: &Row<'_>) -> rusqlite::Result<AnalysisResult> {
    let tags: String = row.get(3)?;
    let frames: String = row.get(11)?;
    Ok(AnalysisResult {
        id: row.get(0)?,
        asset_id: row.get(1)?,
        category: row.get(2)?,
        tags: serde_json::from_str(&tags).unwrap_or_default(),
        quality_score: row.get(4)?,
        has_foreign_language: row.get::<_, i64>(5)? != 0,
        has_large_amount_english_subtitle: row.get::<_, i64>(6)? != 0,
        summary: row.get(7)?,
        recommended_caption: row.get(8)?,
        asr_text: row.get(9)?,
        asr_language: row.get(10)?,
        frame_paths: serde_json::from_str(&frames).unwrap_or_default(),
        model_name: row.get(12)?,
        prompt_version: row.get(13)?,
        created_at: row.get(14)?,
    })
}

impl<'a> AnalysesRepo<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    /// 按 asset_id upsert（每资产至多一条分析结果）。
    pub fn upsert(&self, input: NewAnalysis) -> Result<i64, DomainError> {
        let conn = self.db.conn();
        let tags = serde_json::to_string(&input.tags)?;
        let frames = serde_json::to_string(&input.frame_paths)?;
        conn.execute(
            "INSERT INTO analysis_results
                (asset_id, category, tags_json, quality_score, has_foreign_language,
                 has_large_amount_english_subtitle, summary, recommended_caption,
                 asr_text, asr_language, frame_paths_json, model_name, prompt_version)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
             ON CONFLICT(asset_id) DO UPDATE SET
                category = excluded.category,
                tags_json = excluded.tags_json,
                quality_score = excluded.quality_score,
                has_foreign_language = excluded.has_foreign_language,
                has_large_amount_english_subtitle = excluded.has_large_amount_english_subtitle,
                summary = excluded.summary,
                recommended_caption = excluded.recommended_caption,
                asr_text = excluded.asr_text,
                asr_language = excluded.asr_language,
                frame_paths_json = excluded.frame_paths_json,
                model_name = excluded.model_name,
                prompt_version = excluded.prompt_version",
            params![
                input.asset_id,
                input.category,
                tags,
                input.quality_score,
                input.has_foreign_language as i64,
                input.has_large_amount_english_subtitle as i64,
                input.summary,
                input.recommended_caption,
                input.asr_text,
                input.asr_language,
                frames,
                input.model_name,
                input.prompt_version,
            ],
        )?;
        let id: i64 = conn.query_row(
            "SELECT id FROM analysis_results WHERE asset_id = ?1",
            params![input.asset_id],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    pub fn get_by_asset(&self, asset_id: i64) -> Result<Option<AnalysisResult>, DomainError> {
        let row = self
            .db
            .conn()
            .query_row(
                "SELECT * FROM analysis_results WHERE asset_id = ?1",
                params![asset_id],
                row_to_analysis,
            )
            .optional()?;
        Ok(row)
    }
}
