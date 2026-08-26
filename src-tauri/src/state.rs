use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::agents::filter::FilterRules;
use crate::agents::collector::CollectorAgent;
use crate::agents::content::{ContentAgent, CopyGenerator, OllamaCopyGenerator, OpenAiCopyGenerator};
use crate::agents::runtime::AgentRuntime;
use crate::agents::vision::VisionAgent;
use crate::analysis::asr::WhisperAsr;
use crate::analysis::frames::FfmpegFrameExtractor;
use crate::analysis::llm::{OllamaLlm, OpenAiLlm};
use crate::analysis::openai::OpenAiClient;
use crate::analysis::pipeline::{AnalysisPipeline, Llm, Vision};
use crate::analysis::vision::{OllamaVision, OpenAiVision};
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
        // 登记内置工具目录：run_tool（抽帧/封面/时长探测/音乐合成）优先用内置 ffmpeg/ffprobe。
        crate::util::set_tools_dir(store.tools_dir());
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
    /// vision / llm 各自按 provider 开关选择本地 Ollama 或 OpenAI 兼容云端实现。
    pub fn build_pipeline(&self) -> Result<AnalysisPipeline, DomainError> {
        let s = self.settings.read().expect("settings lock poisoned");
        let script = self.resolve_path(&s.whisper_script);
        // 已安装的模型传本地目录绝对路径（内置基座）；未安装/自定义值原样透传给 faster-whisper。
        let whisper_model = crate::whisper_models::resolve_model_arg(&self.store, &s.whisper_model);
        // 与脚本一致：解释器路径也解析为绝对路径（应用进程 cwd 在 src-tauri 下，相对路径会找不到）。
        let python = self.resolve_path(&s.whisper_python).to_string_lossy().into_owned();
        let vision = Self::build_vision(&s)?;
        let llm = Self::build_llm(&s)?;
        drop(s);

        Ok(AnalysisPipeline::new(
            Arc::new(FfmpegFrameExtractor),
            Arc::new(WhisperAsr::new(script, whisper_model).with_python(python)),
            vision,
            llm,
        ))
    }

    /// 按 vision_provider 装配视觉实现（ollama 本地 / openai 云端）。
    fn build_vision(s: &AppSettings) -> Result<Arc<dyn Vision>, DomainError> {
        if s.vision_provider == "openai" {
            Ok(Arc::new(OpenAiVision {
                client: OpenAiClient::new(&s.openai_vision_base_url, &s.openai_vision_api_key)?,
                model: s.openai_vision_model.clone(),
            }))
        } else {
            Ok(Arc::new(OllamaVision {
                client: AiClient::new(s.ollama_url.clone())?,
                model: s.vision_model.clone(),
            }))
        }
    }

    /// 按 llm_provider 装配文本 LLM 实现（ollama 本地 / openai 云端）。
    fn build_llm(s: &AppSettings) -> Result<Arc<dyn Llm>, DomainError> {
        if s.llm_provider == "openai" {
            Ok(Arc::new(OpenAiLlm {
                client: OpenAiClient::new(&s.openai_llm_base_url, &s.openai_llm_api_key)?,
                model: s.openai_llm_model.clone(),
            }))
        } else {
            Ok(Arc::new(OllamaLlm {
                client: AiClient::new(s.ollama_url.clone())?,
                model: s.llm_model.clone(),
            }))
        }
    }

    /// 按 llm_provider 装配文案生成器（跟随文本 LLM 开关）。
    fn build_copy_generator(s: &AppSettings) -> Result<Arc<dyn CopyGenerator>, DomainError> {
        if s.llm_provider == "openai" {
            Ok(Arc::new(OpenAiCopyGenerator {
                client: OpenAiClient::new(&s.openai_llm_base_url, &s.openai_llm_api_key)?,
                model: s.openai_llm_model.clone(),
            }))
        } else {
            Ok(Arc::new(OllamaCopyGenerator {
                client: AiClient::new(s.ollama_url.clone())?,
                model: s.llm_model.clone(),
            }))
        }
    }

    /// 装配 Agent Runtime 并注册 vision / publisher agent。
    pub fn build_runtime(&self) -> Result<AgentRuntime<'_>, DomainError> {
        let rules = {
            let s = self.settings.read().expect("settings lock poisoned");
            FilterRules {
                quality_min_keep: s.quality_min_keep,
                hit_conditions: s.hit_conditions.clone(),
                exclude_conditions: s.exclude_conditions.clone(),
                condition_basis: s.condition_basis.clone(),
            }
        };
        let (node_bin, script, collector_script, douyin_sync_script) = {
            let s = self.settings.read().expect("settings lock poisoned");
            (
                s.node_bin.clone(),
                self.resolve_path(&s.publisher_script),
                self.resolve_path(&s.collector_script),
                self.resolve_path(&s.douyin_sync_script),
            )
        };
        // yt-dlp 已内置（storage/tools/yt-dlp.exe），不再读设置里的外部路径。
        let ytdlp = crate::tools::ytdlp::exe_path(&self.store)
            .to_string_lossy()
            .into_owned();
        let mut runtime = AgentRuntime::new(&self.db, self.store.clone());
        runtime.register(Box::new(VisionAgent::new(self.build_pipeline()?, rules)));
        runtime.register(Box::new(crate::agents::publisher::PublisherAgent::new(
            node_bin.clone(),
            script,
        )));
        runtime.register(Box::new(CollectorAgent::new(
            node_bin.clone(),
            collector_script,
            ytdlp,
        )));
        runtime.register(Box::new(crate::agents::douyin_sync::DouyinSyncAgent::new(
            node_bin.clone(),
            douyin_sync_script.clone(),
        )));
        runtime.register(Box::new(crate::agents::douyin_moderate::DouyinModerateAgent::new(
            node_bin,
            douyin_sync_script.with_file_name("moderate.js"),
        )));
        let copy_generator = {
            let s = self.settings.read().expect("settings lock poisoned");
            Self::build_copy_generator(&s)?
        };
        runtime.register(Box::new(ContentAgent::new(copy_generator)));
        Ok(runtime)
    }
}
