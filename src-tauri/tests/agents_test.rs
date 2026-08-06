use std::path::PathBuf;
use std::sync::Arc;

use build_douyin_acount_lib::agents::filter::{FilterAgent, FilterRules};
use build_douyin_acount_lib::agents::runtime::AgentRuntime;
use build_douyin_acount_lib::agents::{Agent, AgentOutcome};
use build_douyin_acount_lib::analysis::llm::AnalysisJson;
use build_douyin_acount_lib::analysis::pipeline::{AnalysisPipeline, FrameExtractor, Llm, SynthesisRequest, Vision};
use build_douyin_acount_lib::analysis::{Asr, Transcript};
use build_douyin_acount_lib::db::repos::assets::AssetsRepo;
use build_douyin_acount_lib::db::repos::tasks::TasksRepo;
use build_douyin_acount_lib::agents::AgentContext;
use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::domain::analysis::{ConditionVerdict, FilterVerdict, NewAnalysis};
use build_douyin_acount_lib::domain::asset::{AssetStatus, NewAsset};
use build_douyin_acount_lib::domain::error::DomainError;
use build_douyin_acount_lib::domain::task::TaskStatus;
use build_douyin_acount_lib::media::MediaStore;

fn setup() -> (tempfile::TempDir, Database, MediaStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("dirs");
    let db = Database::open(&dir.path().join("storage/db/app.db")).expect("db");
    (dir, db, store)
}

fn sample_analysis(asset_id: i64) -> NewAnalysis {
    NewAnalysis {
        asset_id,
        category: Some("宠物".to_string()),
        tags: vec!["猫".to_string()],
        quality_score: Some(80),
        has_foreign_language: false,
        has_large_amount_english_subtitle: false,
        summary: Some("一只猫".to_string()),
        recommended_caption: Some("文案".to_string()),
        asr_text: Some("喵喵".to_string()),
        asr_language: Some("zh".to_string()),
        frame_paths: vec![],
        model_name: Some("m".to_string()),
        prompt_version: Some("v1".to_string()),
        condition_results: vec![],
    }
}

// ---- FilterAgent 纯逻辑 ----

#[test]
fn filter_english_content_no_longer_auto_eliminated() {
    let rules = FilterRules::default();
    let a = sample_analysis(1);
    let with_sub = NewAnalysis {
        has_large_amount_english_subtitle: true,
        ..a.clone()
    };
    assert_eq!(FilterAgent::evaluate(&with_sub, &rules).expect("ok").verdict, FilterVerdict::Keep);
    let en = NewAnalysis {
        has_foreign_language: true,
        asr_language: Some("en".to_string()),
        asr_text: Some("welcome to my channel today we cook".to_string()),
        ..a
    };
    assert_eq!(FilterAgent::evaluate(&en, &rules).expect("ok").verdict, FilterVerdict::Keep);
}

#[test]
fn filter_hit_any_keeps() {
    let rules = FilterRules {
        hit_conditions: vec!["宠物".to_string(), "风景".to_string()],
        ..FilterRules::default()
    };
    let a = NewAnalysis {
        condition_results: vec![
            ConditionVerdict { name: "宠物".to_string(), hit: true, basis: "画面".to_string() },
            ConditionVerdict { name: "风景".to_string(), hit: false, basis: "画面".to_string() },
        ],
        ..sample_analysis(1)
    };
    assert_eq!(FilterAgent::evaluate(&a, &rules).expect("ok").verdict, FilterVerdict::Keep);
}

#[test]
fn filter_no_hit_eliminates_with_reason() {
    let rules = FilterRules {
        hit_conditions: vec!["宠物".to_string(), "风景".to_string()],
        ..FilterRules::default()
    };
    let a = NewAnalysis {
        condition_results: vec![
            ConditionVerdict { name: "宠物".to_string(), hit: false, basis: "画面".to_string() },
            ConditionVerdict { name: "风景".to_string(), hit: false, basis: "画面".to_string() },
        ],
        ..sample_analysis(1)
    };
    let d = FilterAgent::evaluate(&a, &rules).expect("ok");
    assert_eq!(d.verdict, FilterVerdict::SkipDownload);
    assert_eq!(d.reason.as_deref(), Some("no_hit_condition"));
}

#[test]
fn filter_exclude_any_eliminates_with_reason() {
    let rules = FilterRules {
        exclude_conditions: vec!["色情".to_string()],
        ..FilterRules::default()
    };
    let a = NewAnalysis {
        condition_results: vec![
            ConditionVerdict { name: "色情".to_string(), hit: true, basis: "画面".to_string() },
        ],
        ..sample_analysis(1)
    };
    let d = FilterAgent::evaluate(&a, &rules).expect("ok");
    assert_eq!(d.verdict, FilterVerdict::SkipDownload);
    assert_eq!(d.reason.as_deref(), Some("excluded_by_condition: 色情"));
}

#[test]
fn filter_exclude_wins_over_hit() {
    let rules = FilterRules {
        hit_conditions: vec!["宠物".to_string()],
        exclude_conditions: vec!["色情".to_string()],
        ..FilterRules::default()
    };
    let a = NewAnalysis {
        condition_results: vec![
            ConditionVerdict { name: "宠物".to_string(), hit: true, basis: "画面".to_string() },
            ConditionVerdict { name: "色情".to_string(), hit: true, basis: "画面".to_string() },
        ],
        ..sample_analysis(1)
    };
    assert_eq!(FilterAgent::evaluate(&a, &rules).expect("ok").verdict, FilterVerdict::SkipDownload);
}

#[test]
fn filter_ignores_blank_conditions() {
    let rules = FilterRules {
        hit_conditions: vec!["  ".to_string(), "宠物".to_string()],
        ..FilterRules::default()
    };
    let a = NewAnalysis {
        condition_results: vec![
            ConditionVerdict { name: "宠物".to_string(), hit: true, basis: "画面".to_string() },
        ],
        ..sample_analysis(1)
    };
    assert_eq!(FilterAgent::evaluate(&a, &rules).expect("ok").verdict, FilterVerdict::Keep);
}

#[test]
fn filter_legacy_analysis_without_verdicts_ignores_conditions() {
    let rules = FilterRules {
        hit_conditions: vec!["宠物".to_string()],
        exclude_conditions: vec!["色情".to_string()],
        ..FilterRules::default()
    };
    assert_eq!(FilterAgent::evaluate(&sample_analysis(1), &rules).expect("ok").verdict, FilterVerdict::Keep);
}

#[test]
fn filter_unjudged_hit_condition_skips_hit_layer() {
    let rules = FilterRules {
        hit_conditions: vec!["宠物".to_string(), "风景".to_string()],
        ..FilterRules::default()
    };
    let a = NewAnalysis {
        condition_results: vec![
            ConditionVerdict { name: "宠物".to_string(), hit: false, basis: "画面".to_string() },
        ],
        ..sample_analysis(1)
    };
    assert_eq!(FilterAgent::evaluate(&a, &rules).expect("ok").verdict, FilterVerdict::Keep);
}

#[test]
fn filter_unjudged_exclude_condition_does_not_exclude() {
    let rules = FilterRules {
        exclude_conditions: vec!["色情".to_string()],
        ..FilterRules::default()
    };
    let a = NewAnalysis {
        condition_results: vec![
            ConditionVerdict { name: "长中文对话".to_string(), hit: true, basis: "音频".to_string() },
        ],
        ..sample_analysis(1)
    };
    assert_eq!(FilterAgent::evaluate(&a, &rules).expect("ok").verdict, FilterVerdict::Keep);
}

#[test]
fn filter_marks_low_quality_as_low_priority() {
    let rules = FilterRules::default();
    let a = NewAnalysis {
        quality_score: Some(40),
        ..sample_analysis(1)
    };
    assert_eq!(FilterAgent::evaluate(&a, &rules).expect("ok").verdict, FilterVerdict::LowPriority);
}

#[test]
fn filter_keeps_clean_content() {
    let rules = FilterRules::default();
    assert_eq!(FilterAgent::evaluate(&sample_analysis(1), &rules).expect("ok").verdict, FilterVerdict::Keep);
}

// ---- Agent Runtime ----

struct FakeOkAgent;
impl Agent for FakeOkAgent {
    fn name(&self) -> &'static str {
        "fake_ok"
    }
    fn run(&self, _ctx: &AgentContext, _task: build_douyin_acount_lib::domain::task::AgentTask) -> Result<AgentOutcome, DomainError> {
        Ok(AgentOutcome::Succeeded(serde_json::json!({"ok": true})))
    }
}

struct FakeFailAgent;
impl Agent for FakeFailAgent {
    fn name(&self) -> &'static str {
        "fake_fail"
    }
    fn run(&self, _ctx: &AgentContext, _task: build_douyin_acount_lib::domain::task::AgentTask) -> Result<AgentOutcome, DomainError> {
        Err(DomainError::InvalidData("boom".to_string()))
    }
}

#[test]
fn runtime_claims_and_succeeds_task() {
    let (dir, db, _store) = setup();
    let mut rt = AgentRuntime::new(&db, MediaStore::new(dir.path().join("storage")));
    rt.register(Box::new(FakeOkAgent));
    let tasks = TasksRepo::new(&db);
    let id = tasks.enqueue("fake_ok", "t", serde_json::json!({}), false).expect("enqueue");
    assert!(rt.run_once("fake_ok").expect("run").is_processed());
    assert_eq!(tasks.get(id).expect("get").expect("some").status, TaskStatus::Succeeded);
    let _ = dir;
}

#[test]
fn runtime_requeues_failed_task_with_backoff() {
    let (dir, db, _store) = setup();
    let mut rt = AgentRuntime::new(&db, MediaStore::new(dir.path().join("storage")));
    rt.register(Box::new(FakeFailAgent));
    let tasks = TasksRepo::new(&db);
    let id = tasks.enqueue("fake_fail", "t", serde_json::json!({}), false).expect("enqueue");
    assert!(rt.run_once("fake_fail").expect("run").is_processed());
    let t = tasks.get(id).expect("get").expect("some");
    assert_eq!(t.status, TaskStatus::Pending);
    assert_eq!(t.attempts, 1);
    assert!(t.next_retry_at.is_some());
    let _ = dir;
}

#[test]
fn runtime_returns_idle_when_no_task() {
    let (dir, db, _store) = setup();
    let rt = AgentRuntime::new(&db, MediaStore::new(dir.path().join("storage")));
    assert!(!rt.run_once("fake_ok").expect("run").is_processed());
    let _ = dir;
}

// ---- VisionAgent 端到端（桩组件） ----

struct FakeExtractor;
impl FrameExtractor for FakeExtractor {
    fn extract(&self, _v: &std::path::Path, _out: &std::path::Path) -> Result<Vec<PathBuf>, DomainError> {
        Ok(vec![PathBuf::from("f1.jpg")])
    }
    fn extract_audio(&self, _v: &std::path::Path, _out: &std::path::Path) -> Result<PathBuf, DomainError> {
        Ok(PathBuf::from("a.wav"))
    }
}

struct FakeAsr;
impl Asr for FakeAsr {
    fn transcribe(&self, _wav: &std::path::Path) -> Result<Transcript, DomainError> {
        Ok(Transcript { text: "喵喵".to_string(), language: "zh".to_string() })
    }
}

struct FakeVision;
impl Vision for FakeVision {
    fn analyze_frame(&self, _p: &std::path::Path) -> Result<build_douyin_acount_lib::analysis::pipeline::FrameInsight, DomainError> {
        Ok(build_douyin_acount_lib::analysis::pipeline::FrameInsight {
            description: "一只猫".to_string(),
            english_text: false,
        })
    }
}

struct FakeLlm;
impl Llm for FakeLlm {
    fn synthesize(&self, _req: SynthesisRequest) -> Result<AnalysisJson, DomainError> {
        Ok(AnalysisJson {
            category: "宠物".to_string(),
            tags: vec!["猫".to_string(), "治愈".to_string()],
            quality_score: 88,
            has_foreign_language: false,
            has_large_amount_english_subtitle: false,
            summary: "一只猫在玩耍".to_string(),
            recommended_caption: "治愈猫猫".to_string(),
            conditions: vec![],
        })
    }
}

#[test]
fn vision_agent_analyzes_asset_and_marks_ready() {
    let (dir, db, store) = setup();
    let video = dir.path().join("v.mp4");
    std::fs::write(&video, b"fake").expect("video");
    let asset_id = AssetsRepo::new(&db)
        .insert(NewAsset {
            dedup_key: Some("tiktok:agent1".to_string()),
            file_path: video.to_string_lossy().into_owned(),
            cover_path: None,
            source_platform: "tiktok".to_string(),
            source_url: None,
            author: None,
            title: Some("猫猫".to_string()),
            description: None,
            published_at: None,
            tags: vec![],
            likes: 0,
            comments: 0,
            plays: 0,
            duration_ms: 5000,
        })
        .expect("insert");

    let pipeline = AnalysisPipeline::new(
        Arc::new(FakeExtractor),
        Arc::new(FakeAsr),
        Arc::new(FakeVision),
        Arc::new(FakeLlm),
    );
    let agent = build_douyin_acount_lib::agents::vision::VisionAgent::new(
        pipeline,
        FilterRules::default(),
    );
    let ctx = AgentContext { db: &db, store };
    let task = build_douyin_acount_lib::domain::task::AgentTask {
        id: 1,
        agent_name: "vision".to_string(),
        task_type: "analyze_asset".to_string(),
        payload: serde_json::json!({"asset_id": asset_id}),
        status: TaskStatus::Running,
        attempts: 1,
        max_attempts: 3,
        next_retry_at: None,
        human_required: false,
        error: None,
        created_at: "t".to_string(),
        updated_at: "t".to_string(),
    };
    let outcome = agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Succeeded(_)));

    let analysis = build_douyin_acount_lib::db::repos::analyses::AnalysesRepo::new(&db)
        .get_by_asset(asset_id)
        .expect("analysis")
        .expect("some");
    assert_eq!(analysis.category.as_deref(), Some("宠物"));
    assert_eq!(analysis.quality_score, Some(88));
    let asset = AssetsRepo::new(&db).get(asset_id).expect("get").expect("some");
    assert_eq!(asset.status, AssetStatus::Candidate);
}

#[test]
fn filter_agent_standalone_updates_status_from_db() {
    let (dir, db, store) = setup();
    let asset_id = AssetsRepo::new(&db)
        .insert(NewAsset {
            dedup_key: Some("tiktok:filter1".to_string()),
            file_path: dir.path().join("v.mp4").to_string_lossy().into_owned(),
            cover_path: None,
            source_platform: "tiktok".to_string(),
            source_url: None,
            author: None,
            title: None,
            description: None,
            published_at: None,
            tags: vec![],
            likes: 0,
            comments: 0,
            plays: 0,
            duration_ms: 1000,
        })
        .expect("insert");
    let en = NewAnalysis {
        condition_results: vec![
            ConditionVerdict { name: "色情".to_string(), hit: true, basis: "画面".to_string() },
        ],
        ..sample_analysis(asset_id)
    };
    build_douyin_acount_lib::db::repos::analyses::AnalysesRepo::new(&db)
        .upsert(en)
        .expect("upsert");
    let ctx = AgentContext { db: &db, store };
    let agent = FilterAgent::new(FilterRules {
        exclude_conditions: vec!["色情".to_string()],
        ..FilterRules::default()
    });
    let task = build_douyin_acount_lib::domain::task::AgentTask {
        id: 2,
        agent_name: "filter".to_string(),
        task_type: "apply_filter".to_string(),
        payload: serde_json::json!({"asset_id": asset_id}),
        status: TaskStatus::Running,
        attempts: 1,
        max_attempts: 3,
        next_retry_at: None,
        human_required: false,
        error: None,
        created_at: "t".to_string(),
        updated_at: "t".to_string(),
    };
    agent.run(&ctx, task).expect("run");
    let asset = AssetsRepo::new(&db).get(asset_id).expect("get").expect("some");
    assert_eq!(asset.status, AssetStatus::Eliminated);
}
