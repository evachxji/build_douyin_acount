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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use build_douyin_acount_lib::agents::filter::FilterAgent;
use build_douyin_acount_lib::analysis::frames::FfmpegFrameExtractor;
use build_douyin_acount_lib::analysis::llm::OllamaLlm;
use build_douyin_acount_lib::analysis::pipeline::AnalysisPipeline;
use build_douyin_acount_lib::analysis::vision::OllamaVision;
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

    println!("video: {}", video.display());
    println!("ollama: {ollama_url} | vision: {vision_model} | llm: {llm_model}");
    println!("whisper: {python} {script} ({whisper_model})");

    let vision_client = AiClient::new(ollama_url.clone()).expect("ai client");
    let llm_client = AiClient::new(ollama_url).expect("ai client");
    let pipeline = AnalysisPipeline::new(
        Arc::new(FfmpegFrameExtractor),
        Arc::new(WhisperAsr::new(script, whisper_model).with_python(python)),
        Arc::new(OllamaVision {
            client: vision_client,
            model: vision_model,
        }),
        Arc::new(OllamaLlm {
            client: llm_client,
            model: llm_model,
        }),
    );

    let work = std::env::temp_dir().join(format!("e2e_analysis_{}", std::process::id()));
    let analysis = pipeline.analyze(&video, &work, 0, Some("E2E 测试视频")).expect("analyze");
    let verdict = FilterAgent::evaluate(&analysis, &Default::default()).expect("filter");
    let output = serde_json::json!({
        "verdict": verdict.as_str(),
        "analysis": analysis,
    });
    println!("=== RESULT ===");
    println!("{}", serde_json::to_string_pretty(&output).expect("json"));
}
