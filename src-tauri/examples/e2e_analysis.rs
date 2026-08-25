//! 真实环境端到端联调示例：
//! cargo run --example e2e_analysis -- <video.mp4>
//!
//! 环境变量（均可选）：
//!   OLLAMA_URL     默认 http://localhost:11434
//!   VISION_MODEL   默认 qwen2.5vl:7b
//!   LLM_MODEL      默认 qwen2.5:7b
//!   WHISPER_PYTHON 默认 scripts/whisper-server/.venv/Scripts/python.exe
//!   WHISPER_SCRIPT 默认 scripts/whisper-server/whisper_transcribe.py
//!   WHISPER_MODEL  默认 base
//!   VISION_PROVIDER / LLM_PROVIDER  默认 ollama；设 openai 走云端 OpenAI 兼容接口
//!   OPENAI_BASE_URL 默认 https://api.kimi.com/coding/v1
//!   OPENAI_API_KEY  云端必填
//!   OPENAI_VISION_MODEL / OPENAI_LLM_MODEL  默认 k3-256k

use std::path::{Path, PathBuf};
use std::sync::Arc;

use build_douyin_acount_lib::agents::filter::FilterAgent;
use build_douyin_acount_lib::analysis::frames::FfmpegFrameExtractor;
use build_douyin_acount_lib::analysis::llm::{OllamaLlm, OpenAiLlm};
use build_douyin_acount_lib::analysis::openai::OpenAiClient;
use build_douyin_acount_lib::analysis::pipeline::{AnalysisPipeline, Llm, Vision};
use build_douyin_acount_lib::analysis::vision::{OllamaVision, OpenAiVision};
use build_douyin_acount_lib::analysis::asr::WhisperAsr;
use build_douyin_acount_lib::analysis::AiClient;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn main() {
    let video_arg = std::env::args().nth(1).expect("usage: e2e_analysis <video.mp4>");
    let video = PathBuf::from(&video_arg);
    assert!(video.is_file(), "video not found: {video_arg}");

    let base = env_or("CARGO_MANIFEST_DIR", ".");
    let repo_root = Path::new(&base).parent().expect("repo root");
    let python = env_or(
        "WHISPER_PYTHON",
        &repo_root.join("scripts/whisper-server/.venv/Scripts/python.exe").to_string_lossy(),
    );
    let script = env_or(
        "WHISPER_SCRIPT",
        &repo_root.join("scripts/whisper-server/whisper_transcribe.py").to_string_lossy(),
    );
    let whisper_model = env_or("WHISPER_MODEL", "base");
    let ollama_url = env_or("OLLAMA_URL", "http://localhost:11434");
    let vision_model = env_or("VISION_MODEL", "qwen2.5vl:7b");
    let llm_model = env_or("LLM_MODEL", "qwen2.5:7b");
    let vision_provider = env_or("VISION_PROVIDER", "ollama");
    let llm_provider = env_or("LLM_PROVIDER", "ollama");
    let openai_base_url = env_or("OPENAI_BASE_URL", "https://api.kimi.com/coding/v1");
    let openai_api_key = env_or("OPENAI_API_KEY", "");
    let openai_vision_model = env_or("OPENAI_VISION_MODEL", "k3-256k");
    let openai_llm_model = env_or("OPENAI_LLM_MODEL", "k3-256k");

    println!("video: {}", video.display());
    println!("vision provider: {vision_provider} | llm provider: {llm_provider}");

    let vision: Arc<dyn Vision> = if vision_provider == "openai" {
        Arc::new(OpenAiVision {
            client: OpenAiClient::new(&openai_base_url, &openai_api_key).expect("vision client"),
            model: openai_vision_model,
        })
    } else {
        Arc::new(OllamaVision {
            client: AiClient::new(ollama_url.clone()).expect("ai client"),
            model: vision_model,
        })
    };
    let llm: Arc<dyn Llm> = if llm_provider == "openai" {
        Arc::new(OpenAiLlm {
            client: OpenAiClient::new(&openai_base_url, &openai_api_key).expect("llm client"),
            model: openai_llm_model,
        })
    } else {
        Arc::new(OllamaLlm {
            client: AiClient::new(ollama_url).expect("ai client"),
            model: llm_model,
        })
    };
    let pipeline = AnalysisPipeline::new(
        Arc::new(FfmpegFrameExtractor),
        Arc::new(WhisperAsr::new(script, whisper_model).with_python(python)),
        vision,
        llm,
    );

    let work = std::env::temp_dir().join(format!("e2e_analysis_{}", std::process::id()));
    let analysis = pipeline
        .analyze(&video, &work, 0, Some("E2E 测试视频"), &[], &[], &std::collections::HashMap::new())
        .expect("analyze");
    let decision = FilterAgent::evaluate(&analysis, &Default::default()).expect("filter");
    let verdict = decision.verdict;
    let output = serde_json::json!({
        "verdict": verdict.as_str(),
        "analysis": analysis,
    });
    println!("=== RESULT ===");
    println!("{}", serde_json::to_string_pretty(&output).expect("json"));
}
