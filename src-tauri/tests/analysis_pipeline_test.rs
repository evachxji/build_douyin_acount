use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use build_douyin_acount_lib::analysis::frames::{frame_count_for_duration, FfmpegFrameExtractor};
use build_douyin_acount_lib::analysis::llm::{parse_analysis_json, AnalysisJson};
use build_douyin_acount_lib::analysis::pipeline::{AnalysisPipeline, FrameExtractor, Llm, SynthesisRequest, Vision};
use build_douyin_acount_lib::analysis::{Asr, Transcript};
use build_douyin_acount_lib::domain::analysis::NewAnalysis;
use build_douyin_acount_lib::domain::error::DomainError;

fn has_tool(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn make_test_video(dir: &std::path::Path) -> Option<PathBuf> {
    if !has_tool("ffmpeg") {
        return None;
    }
    let out = dir.join("sample.mp4");
    let ok = Command::new("ffmpeg")
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=6:size=320x240:rate=10",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&out)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    ok.then_some(out)
}

#[test]
fn frame_count_scales_with_duration_within_bounds() {
    assert_eq!(frame_count_for_duration(0), 3);
    assert_eq!(frame_count_for_duration(5_000), 3);
    assert_eq!(frame_count_for_duration(30_000), 4);
    assert_eq!(frame_count_for_duration(60_000), 5);
    assert_eq!(frame_count_for_duration(120_000), 6);
    assert_eq!(frame_count_for_duration(600_000), 6);
}

#[test]
fn parse_analysis_json_accepts_valid_output() {
    let raw = r#"{
        "category": "宠物",
        "tags": ["猫", "治愈"],
        "quality_score": 85,
        "has_foreign_language": false,
        "has_large_amount_english_subtitle": true,
        "summary": "一只猫咪玩耍",
        "recommended_caption": "快来看看这只猫"
    }"#;
    let parsed = parse_analysis_json(raw).expect("parse");
    assert_eq!(parsed.category, "宠物");
    assert_eq!(parsed.tags, vec!["猫", "治愈"]);
    assert_eq!(parsed.quality_score, 85);
    assert!(parsed.has_large_amount_english_subtitle);
}

#[test]
fn parse_analysis_json_parses_conditions() {
    let raw = r#"{
        "category": "宠物",
        "tags": ["猫"],
        "quality_score": 85,
        "has_foreign_language": false,
        "has_large_amount_english_subtitle": false,
        "summary": "一只猫",
        "recommended_caption": "看猫",
        "conditions": [
            {"name": "宠物", "hit": true, "basis": "画面"},
            {"name": "长中文对话", "hit": false, "basis": "音频"}
        ]
    }"#;
    let parsed = parse_analysis_json(raw).expect("parse");
    assert_eq!(parsed.conditions.len(), 2);
    assert_eq!(parsed.conditions[0].name, "宠物");
    assert!(parsed.conditions[0].hit);
    assert_eq!(parsed.conditions[0].basis, "画面");
    assert!(!parsed.conditions[1].hit);
}

#[test]
fn parse_analysis_json_without_conditions_defaults_empty() {
    let raw = r#"{"category":"宠物","tags":[],"quality_score":80,"has_foreign_language":false,"has_large_amount_english_subtitle":false,"summary":"s","recommended_caption":"c"}"#;
    let parsed = parse_analysis_json(raw).expect("parse");
    assert!(parsed.conditions.is_empty());
}

#[test]
fn parse_analysis_json_tolerates_invalid_conditions() {
    let raw = r#"{"category":"宠物","tags":[],"quality_score":80,"has_foreign_language":false,"has_large_amount_english_subtitle":false,"summary":"s","recommended_caption":"c","conditions":"not-an-array"}"#;
    let parsed = parse_analysis_json(raw).expect("parse");
    assert!(parsed.conditions.is_empty());
}

#[test]
fn parse_analysis_json_rejects_missing_fields() {
    let raw = r#"{"category": "宠物"}"#;
    let err = parse_analysis_json(raw).expect_err("missing fields must fail");
    assert!(err.to_string().contains("quality_score"), "got: {err}");
}

#[test]
fn ffmpeg_frame_extractor_produces_expected_frames() {
    let dir = tempfile::tempdir().expect("tempdir");
    let Some(video) = make_test_video(dir.path()) else {
        eprintln!("skipped: ffmpeg not available");
        return;
    };
    let out_dir = dir.path().join("frames");
    let extractor = FfmpegFrameExtractor;
    let frames = extractor.extract(&video, &out_dir).expect("extract");
    assert!(!frames.is_empty(), "frames produced");
    assert!(frames.iter().all(|p| p.exists()), "all frames exist");
}

// ---- 桩实现：验证 Pipeline 编排 ----

struct FakeExtractor;
impl FrameExtractor for FakeExtractor {
    fn extract(&self, _video: &std::path::Path, _out_dir: &std::path::Path) -> Result<Vec<PathBuf>, DomainError> {
        Ok(vec![PathBuf::from("frame_1.jpg"), PathBuf::from("frame_2.jpg")])
    }
    fn extract_audio(&self, _video: &std::path::Path, _out_dir: &std::path::Path) -> Result<PathBuf, DomainError> {
        Ok(PathBuf::from("audio.wav"))
    }
}

struct FakeAsr;
impl Asr for FakeAsr {
    fn transcribe(&self, _wav: &std::path::Path) -> Result<Transcript, DomainError> {
        Ok(Transcript {
            text: "hello world".to_string(),
            language: "en".to_string(),
        })
    }
}

struct FakeVision;
impl Vision for FakeVision {
    fn analyze_frame(&self, _path: &std::path::Path) -> Result<build_douyin_acount_lib::analysis::pipeline::FrameInsight, DomainError> {
        Ok(build_douyin_acount_lib::analysis::pipeline::FrameInsight {
            description: "一只猫在镜头前".to_string(),
            english_text: false,
        })
    }
}

struct FakeLlm;
impl Llm for FakeLlm {
    fn synthesize(&self, req: SynthesisRequest) -> Result<AnalysisJson, DomainError> {
        Ok(AnalysisJson {
            category: "宠物".to_string(),
            tags: vec!["猫".to_string()],
            quality_score: 90,
            has_foreign_language: req.asr_language.as_deref() == Some("en"),
            has_large_amount_english_subtitle: req.frames.iter().any(|f| f.english_text),
            summary: req.frames.first().map(|f| f.description.clone()).unwrap_or_default(),
            recommended_caption: "测试文案".to_string(),
            conditions: req
                .hit_conditions
                .iter()
                .map(|c| build_douyin_acount_lib::domain::analysis::ConditionVerdict {
                    name: c.clone(),
                    hit: true,
                    basis: "画面".to_string(),
                })
                .collect(),
        })
    }
}

#[test]
fn pipeline_orchestrates_frames_asr_vision_llm_into_analysis() {
    let dir = tempfile::tempdir().expect("tempdir");
    let video = dir.path().join("v.mp4");
    std::fs::write(&video, b"fake").expect("write");
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).expect("workdir");

    let pipeline = AnalysisPipeline::new(
        Arc::new(FakeExtractor),
        Arc::new(FakeAsr),
        Arc::new(FakeVision),
        Arc::new(FakeLlm),
    );
    let result: NewAnalysis = pipeline
        .analyze(&video, &work, 42, None, &["宠物".to_string()], &[])
        .expect("analyze");
    assert_eq!(result.category.as_deref(), Some("宠物"));
    assert_eq!(result.asr_text.as_deref(), Some("hello world"));
    assert!(result.has_foreign_language, "en asr flags foreign language");
    assert_eq!(result.quality_score, Some(90));
    assert_eq!(result.condition_results.len(), 1);
    assert_eq!(result.condition_results[0].name, "宠物");
    assert!(result.condition_results[0].hit);
}

#[test]
fn pipeline_propagates_extraction_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let video = dir.path().join("missing.mp4");
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).expect("workdir");
    std::fs::write(&video, b"fake").expect("write");
    let pipeline = AnalysisPipeline::new(
        Arc::new(AlwaysFailExtractor),
        Arc::new(FakeAsr),
        Arc::new(FakeVision),
        Arc::new(FakeLlm),
    );
    let err = pipeline.analyze(&video, &work, 42, None, &[], &[]).expect_err("must fail");
    assert!(err.to_string().contains("extract failed"), "got: {err}");
}

struct AlwaysFailExtractor;
impl FrameExtractor for AlwaysFailExtractor {
    fn extract(&self, _video: &std::path::Path, _out_dir: &std::path::Path) -> Result<Vec<PathBuf>, DomainError> {
        Err(DomainError::InvalidData("extract failed".to_string()))
    }
}

// ---- AiClient 与 Ollama 协议：本地 mock 服务器 ----

fn spawn_mock_llm(body: String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn ai_client_parses_ollama_chat_response() {
    let body = r#"{"model":"m","created_at":"t","message":{"role":"assistant","content":"{\"category\":\"美食\",\"tags\":[],\"quality_score\":70}"},"done":true}"#.to_string();
    let base = spawn_mock_llm(body);
    let client = build_douyin_acount_lib::analysis::AiClient::new(base).expect("client");
    let content = client
        .chat("qwen2.5:7b", vec![], Some("json"))
        .await
        .expect("chat");
    assert!(content.contains("美食"), "got: {content}");
}

#[tokio::test]
async fn ai_client_surfaces_http_errors() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    let client = build_douyin_acount_lib::analysis::AiClient::new(format!("http://{addr}")).expect("client");
    let err = client.chat("m", vec![], None).await.expect_err("500 must fail");
    assert!(err.to_string().contains("500"), "got: {err}");
}
