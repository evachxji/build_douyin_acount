use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::agents::filter::FilterRules;
use crate::agents::collector::CollectorAgent;
use crate::agents::runtime::AgentRuntime;
use crate::agents::vision::VisionAgent;
use crate::analysis::asr::WhisperAsr;
use crate::analysis::frames::FfmpegFrameExtractor;
use crate::analysis::llm::OllamaLlm;
use crate::analysis::pipeline::AnalysisPipeline;
use crate::analysis::vision::OllamaVision;
use crate::analysis::AiClient;
use crate::db::Database;
use crate::domain::error::DomainError;
use crate::media::MediaStore;
use crate::settings::AppSettings;

/// Tauri 托管的应用状态：数据库、媒体存储、运行时设置。
/// 不依赖 tauri 类型，便于单元测试与后台 worker 复用。
pub struct AppState {
    pub db: Arc<Database>,
    pub store: MediaStore,
    pub settings: Arc<RwLock<AppSettings>>,
}

impl AppState {
    pub fn new(db: Database, store: MediaStore, settings: AppSettings) -> Self {
        Self {
            db: Arc::new(db),
            store,
            settings: Arc::new(RwLock::new(settings)),
        }
    }

    /// 相对路径按当前目录解析；cargo run 的 cwd 是 src-tauri 时回退到仓库根。
    fn resolve_path(&self, p: &str) -> PathBuf {
        let path = Path::new(p);
        if path.exists() {
            return path.to_path_buf();
        }
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Some(parent) = manifest.parent() {
            let candidate = parent.join(p);
            if candidate.exists() {
                return candidate;
            }
        }
        std::env::current_dir()
            .unwrap_or_default()
            .join(p)
    }

    /// 按当前设置装配完整分析管线（抽帧 → Whisper → Vision → LLM）。
    pub fn build_pipeline(&self) -> Result<AnalysisPipeline, DomainError> {
        let s = self.settings.read().expect("settings lock poisoned");
        let script = self.resolve_path(&s.whisper_script);
        let whisper_model = s.whisper_model.clone();
        let python = s.whisper_python.clone();
        let vision_model = s.vision_model.clone();
        let llm_model = s.llm_model.clone();
        let ollama_url = s.ollama_url.clone();
        drop(s);

        Ok(AnalysisPipeline::new(
            Arc::new(FfmpegFrameExtractor),
            Arc::new(WhisperAsr::new(script, whisper_model).with_python(python)),
            Arc::new(OllamaVision {
                client: AiClient::new(ollama_url.clone())?,
                model: vision_model,
            }),
            Arc::new(OllamaLlm {
                client: AiClient::new(ollama_url)?,
                model: llm_model,
            }),
        ))
    }

    /// 装配 Agent Runtime 并注册 vision / publisher agent。
    pub fn build_runtime(&self) -> Result<AgentRuntime<'_>, DomainError> {
        let rules = {
            let s = self.settings.read().expect("settings lock poisoned");
            FilterRules {
                english_ratio_threshold: s.english_ratio_threshold,
                quality_min_keep: s.quality_min_keep,
            }
        };
        let (node_bin, script, collector_script, ytdlp) = {
            let s = self.settings.read().expect("settings lock poisoned");
            (
                s.node_bin.clone(),
                self.resolve_path(&s.publisher_script),
                self.resolve_path(&s.collector_script),
                s.ytdlp_path.clone(),
            )
        };
        let mut runtime = AgentRuntime::new(&self.db, self.store.clone());
        runtime.register(Box::new(VisionAgent::new(self.build_pipeline()?, rules)));
        runtime.register(Box::new(crate::agents::publisher::PublisherAgent::new(
            node_bin.clone(),
            script,
        )));
        runtime.register(Box::new(CollectorAgent::new(node_bin, collector_script, ytdlp)));
        Ok(runtime)
    }
}
