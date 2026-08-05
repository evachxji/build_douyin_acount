//! Content Agent + 推荐器测试：假 LLM 生成文案、打分推荐写库并入队文案生成。
use std::sync::Arc;

use build_douyin_acount_lib::agents::content::{ContentAgent, CopyGenerator};
use build_douyin_acount_lib::agents::{Agent, AgentContext, AgentOutcome};
use build_douyin_acount_lib::commands::content::recommend;
use build_douyin_acount_lib::db::repos::analyses::AnalysesRepo;
use build_douyin_acount_lib::db::repos::assets::AssetsRepo;
use build_douyin_acount_lib::db::repos::content::ContentRepo;
use build_douyin_acount_lib::db::repos::tasks::TasksRepo;
use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::domain::analysis::NewAnalysis;
use build_douyin_acount_lib::domain::asset::{AssetStatus, NewAsset};
use build_douyin_acount_lib::domain::error::DomainError;
use build_douyin_acount_lib::media::MediaStore;
use serde_json::json;

struct FakeGenerator;

impl CopyGenerator for FakeGenerator {
    fn generate_copy(
        &self,
        _style_prompt: &str,
        _analysis_context: &str,
    ) -> Result<String, DomainError> {
        Ok(r#"{"title":"谁能拒绝这样一只困困的小猫","body":"每天最幸福的事情就是看到它的小表情","hashtags":["萌宠","猫咪","治愈"]}"#
            .to_string())
    }
}

fn test_db() -> Database {
    let dir = tempfile::tempdir().expect("tempdir");
    Database::open(&dir.path().join("test.db")).expect("db")
}

fn insert_asset(db: &Database, title: &str, author: &str, likes: i64) -> i64 {
    let repo = AssetsRepo::new(db);
    let id = repo
        .insert(NewAsset {
            dedup_key: Some(format!("content-{}", uuid::Uuid::new_v4())),
            file_path: "videos/x.mp4".to_string(),
            cover_path: None,
            source_platform: "tiktok".to_string(),
            source_url: None,
            author: Some(author.to_string()),
            title: Some(title.to_string()),
            description: None,
            published_at: None,
            tags: Vec::new(),
            likes,
            comments: 0,
            plays: 0,
            duration_ms: 1000,
        })
        .expect("insert");
    repo.update_status(id, AssetStatus::Analyzing).expect("analyzing");
    repo.update_status(id, AssetStatus::Ready).expect("ready");
    id
}

fn attach_analysis(db: &Database, asset_id: i64, category: &str, quality: i64) {
    AnalysesRepo::new(db)
        .upsert(NewAnalysis {
            asset_id,
            category: Some(category.to_string()),
            tags: vec!["猫".to_string()],
            quality_score: Some(quality),
            has_foreign_language: false,
            has_large_amount_english_subtitle: false,
            summary: Some("一只猫咪".to_string()),
            recommended_caption: Some("推荐文案".to_string()),
            asr_text: None,
            asr_language: None,
            frame_paths: Vec::new(),
            model_name: Some("test".to_string()),
            prompt_version: Some("t".to_string()),
        })
        .expect("analysis");
}

#[test]
fn content_agent_generates_and_stores_copy() {
    let db = test_db();
    let asset_id = insert_asset(&db, "猫咪打哈欠", "cat", 0);
    attach_analysis(&db, asset_id, "宠物", 90);
    let task_id = TasksRepo::new(&db)
        .enqueue(
            "content",
            "generate_copy",
            json!({ "asset_id": asset_id, "style": "治愈" }),
            false,
        )
        .expect("enqueue");
    let task = TasksRepo::new(&db).claim_next("content").expect("claim").expect("task");
    assert_eq!(task.id, task_id);
    let agent = ContentAgent::new(Arc::new(FakeGenerator));
    let store = MediaStore::new(std::env::temp_dir().join("content-test"));
    let ctx = AgentContext { db: &db, store };
    let outcome = agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Succeeded(_)));
    let copy = ContentRepo::new(&db).get_by_asset(asset_id).expect("get").expect("copy");
    assert_eq!(copy.title, "谁能拒绝这样一只困困的小猫");
    assert_eq!(copy.hashtags, vec!["萌宠", "猫咪", "治愈"]);
}

#[test]
fn recommender_scores_groups_and_enqueues_copy_tasks() {
    let db = test_db();
    // 宠物：两条不同作者；美食：一条
    let a1 = insert_asset(&db, "猫咪", "author_a", 500_000);
    let a2 = insert_asset(&db, "狗狗", "author_b", 10_000);
    let a3 = insert_asset(&db, "蛋吐司", "author_c", 200_000);
    attach_analysis(&db, a1, "宠物", 95);
    attach_analysis(&db, a2, "宠物", 60);
    attach_analysis(&db, a3, "美食", 80);

    let result = recommend(&db, "治愈").expect("recommend");
    let groups = result["groups"].as_array().expect("groups");
    let pet = groups
        .iter()
        .find(|g| g["category"] == "宠物")
        .expect("pet group");
    let pet_items = pet["items"].as_array().expect("items");
    assert!(!pet_items.is_empty());
    // 高质量 + 高点赞的排在前面
    assert_eq!(pet_items[0]["asset_id"], a1);
    // 多样性：a1 与 a2 作者不同都入选
    assert!(pet_items.iter().any(|i| i["asset_id"] == a2));
    // recommendations 已写库
    let count: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM recommendations", [], |r| r.get(0))
        .expect("count");
    assert!(count >= 3);
    // 文案生成任务已入队
    let content_tasks = TasksRepo::new(&db)
        .list(None)
        .expect("tasks")
        .into_iter()
        .filter(|t| t.agent_name == "content")
        .count();
    assert_eq!(content_tasks, 3);
}

#[test]
fn account_styles_are_seeded() {
    let db = test_db();
    let styles = build_douyin_acount_lib::db::repos::content::StylesRepo::new(&db)
        .list()
        .expect("styles");
    let names: Vec<String> = styles.iter().map(|s| s.name.clone()).collect();
    assert!(names.contains(&"搞笑".to_string()));
    assert!(names.contains(&"治愈".to_string()));
    assert!(names.contains(&"高级感".to_string()));
    assert!(names.contains(&"情绪价值".to_string()));
}
