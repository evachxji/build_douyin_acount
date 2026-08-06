use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::analysis::NewAnalysis;
use crate::domain::error::DomainError;

/// 单帧分析结论。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct FrameInsight {
    pub description: String,
    /// 画面中是否出现较大面积英文文字/字幕。
    pub english_text: bool,
}

/// LLM 综合判断的输入。
#[derive(Debug, Clone)]
pub struct SynthesisRequest {
    pub frames: Vec<FrameInsight>,
    pub asr_text: Option<String>,
    pub asr_language: Option<String>,
    pub title: Option<String>,
    pub hit_conditions: Vec<String>,
    pub exclude_conditions: Vec<String>,
}

/// 帧提取能力。
pub trait FrameExtractor: Send + Sync {
    fn extract(&self, video: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, DomainError>;
    /// 默认不支持音频提取（桩实现可跳过）。
    fn extract_audio(&self, _video: &Path, _out_dir: &Path) -> Result<PathBuf, DomainError> {
        Err(DomainError::InvalidData("audio extraction not supported".to_string()))
    }
}

/// 视觉理解能力。
pub trait Vision: Send + Sync {
    fn analyze_frame(&self, path: &Path) -> Result<FrameInsight, DomainError>;
}

/// LLM 综合能力。
pub trait Llm: Send + Sync {
    fn synthesize(&self, req: SynthesisRequest) -> Result<crate::analysis::llm::AnalysisJson, DomainError>;
}

/// 分析管线编排：抽帧 -> ASR -> Vision -> LLM 综合。
pub struct AnalysisPipeline {
    frames: Arc<dyn FrameExtractor>,
    asr: Arc<dyn super::Asr>,
    vision: Arc<dyn Vision>,
    llm: Arc<dyn Llm>,
}

impl AnalysisPipeline {
    pub fn new(
        frames: Arc<dyn FrameExtractor>,
        asr: Arc<dyn super::Asr>,
        vision: Arc<dyn Vision>,
        llm: Arc<dyn Llm>,
    ) -> Self {
        Self {
            frames,
            asr,
            vision,
            llm,
        }
    }

    /// 对视频执行完整分析，返回可入库的分析结果。
    pub fn analyze(
        &self,
        video: &Path,
        work_dir: &Path,
        asset_id: i64,
        title: Option<&str>,
        hit_conditions: &[String],
        exclude_conditions: &[String],
    ) -> Result<NewAnalysis, DomainError> {
        if !video.is_file() {
            return Err(DomainError::InvalidData(format!("video not found: {}", video.display())));
        }
        let frames_dir = work_dir.join("frames");
        let audio_dir = work_dir.join("audio");
        let frame_paths = self.frames.extract(video, &frames_dir)?;

        // 音频提取失败不阻塞分析（静音/无音轨视频继续走视觉路径）。
        let audio = match self.frames.extract_audio(video, &audio_dir) {
            Ok(a) => Some(a),
            Err(e) => {
                tracing::warn!("audio extraction skipped: {e}");
                None
            }
        };
        let transcript = match audio {
            Some(wav) => match self.asr.transcribe(&wav) {
                Ok(t) => Some(t),
                Err(e) => {
                    tracing::warn!("asr failed, continue without transcript: {e}");
                    None
                }
            },
            None => None,
        };

        let mut insights = Vec::with_capacity(frame_paths.len());
        for p in &frame_paths {
            insights.push(self.vision.analyze_frame(p)?);
        }
        let json = self.llm.synthesize(SynthesisRequest {
            frames: insights,
            asr_text: transcript.as_ref().map(|t| t.text.clone()),
            asr_language: transcript.as_ref().map(|t| t.language.clone()),
            title: title.map(|s| s.to_string()),
            hit_conditions: hit_conditions.to_vec(),
            exclude_conditions: exclude_conditions.to_vec(),
        })?;
        Ok(NewAnalysis {
            asset_id,
            category: Some(json.category),
            tags: json.tags,
            quality_score: Some(json.quality_score),
            has_foreign_language: json.has_foreign_language,
            has_large_amount_english_subtitle: json.has_large_amount_english_subtitle,
            summary: Some(json.summary),
            recommended_caption: Some(json.recommended_caption),
            asr_text: transcript.as_ref().map(|t| t.text.clone()),
            asr_language: transcript.as_ref().map(|t| t.language.clone()),
            frame_paths: frame_paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            model_name: None,
            prompt_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            condition_results: json.conditions,
        })
    }
}
