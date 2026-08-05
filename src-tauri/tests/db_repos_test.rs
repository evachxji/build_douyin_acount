use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::db::repos::assets::{AssetFilter, AssetsRepo};
use build_douyin_acount_lib::db::repos::analyses::AnalysesRepo;
use build_douyin_acount_lib::db::repos::tasks::TasksRepo;
use build_douyin_acount_lib::domain::analysis::NewAnalysis;
use build_douyin_acount_lib::domain::asset::{AssetStatus, NewAsset};
use build_douyin_acount_lib::domain::task::TaskStatus;

fn test_db() -> Database {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("test.db");
    Database::open(&path).expect("open db")
}

fn sample_asset(title: &str) -> NewAsset {
    NewAsset {
        dedup_key: Some(format!("tiktok:{}", title)),
        file_path: format!("videos/{}.mp4", title),
        cover_path: None,
        source_platform: "tiktok".to_string(),
        source_url: Some(format!("https://tiktok.com/{}", title)),
        author: Some("author_a".to_string()),
        title: Some(title.to_string()),
        description: None,
        published_at: None,
        tags: vec!["猫".to_string(), "治愈".to_string()],
        likes: 100,
        comments: 10,
        plays: 1000,
        duration_ms: 15_000,
    }
}

#[test]
fn migrations_create_all_tables() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("app.db");
    let db = Database::open(&path).expect("open db");
    let tables = db
        .conn()
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    for expected in [
        "account_styles",
        "agent_tasks",
        "analysis_results",
        "app_settings",
        "publish_drafts",
        "recommendations",
        "task_logs",
        "video_assets",
    ] {
        assert!(tables.iter().any(|t| t == expected), "missing table {expected}");
    }
}

#[test]
fn asset_insert_get_roundtrip() {
    let db = test_db();
    let repo = AssetsRepo::new(&db);
    let id = repo.insert(sample_asset("hello")).expect("insert");
    let got = repo.get(id).expect("get").expect("some");
    assert_eq!(got.title.as_deref(), Some("hello"));
    assert_eq!(got.status, AssetStatus::New);
    assert_eq!(got.tags, vec!["猫".to_string(), "治愈".to_string()]);
    assert_eq!(got.likes, 100);
}

#[test]
fn asset_dedup_key_is_unique() {
    let db = test_db();
    let repo = AssetsRepo::new(&db);
    repo.insert(sample_asset("same")).expect("first");
    let err = repo.insert(sample_asset("same")).expect_err("dup must fail");
    assert!(err.to_string().contains("UNIQUE"), "got: {err}");
}

#[test]
fn asset_find_by_dedup() {
    let db = test_db();
    let repo = AssetsRepo::new(&db);
    repo.insert(sample_asset("x")).expect("insert");
    let found = repo.find_by_dedup("tiktok:x").expect("query").expect("some");
    assert_eq!(found.title.as_deref(), Some("x"));
    assert!(repo.find_by_dedup("tiktok:nope").expect("query").is_none());
}

#[test]
fn asset_status_transition_validated_on_update() {
    let db = test_db();
    let repo = AssetsRepo::new(&db);
    let id = repo.insert(sample_asset("t")).expect("insert");
    repo.update_status(id, AssetStatus::Analyzing).expect("ok transition");
    let err = repo
        .update_status(id, AssetStatus::Published)
        .expect_err("illegal transition must fail");
    assert!(err.to_string().contains("transition"), "got: {err}");
    repo.update_status(id, AssetStatus::Ready).expect("ok");
}

#[test]
fn asset_favorite_toggle_and_filter() {
    let db = test_db();
    let repo = AssetsRepo::new(&db);
    let id = repo.insert(sample_asset("fav")).expect("insert");
    repo.set_favorite(id, true).expect("fav on");
    let favs = repo.list(AssetFilter { favorite: Some(true), ..Default::default() }).expect("list");
    assert_eq!(favs.len(), 1);
    repo.set_favorite(id, false).expect("fav off");
    let favs = repo.list(AssetFilter { favorite: Some(true), ..Default::default() }).expect("list");
    assert!(favs.is_empty());
}

#[test]
fn asset_list_filters_by_status_and_category() {
    let db = test_db();
    let assets = AssetsRepo::new(&db);
    let analyses = AnalysesRepo::new(&db);
    let id = assets.insert(sample_asset("cat_video")).expect("insert");
    assets.update_status(id, AssetStatus::Analyzing).expect("analyzing");
    assets.update_status(id, AssetStatus::Ready).expect("ready");
    analyses.upsert(NewAnalysis {
        asset_id: id,
        category: Some("宠物".to_string()),
        tags: vec!["猫".to_string()],
        quality_score: Some(85),
        has_foreign_language: false,
        has_large_amount_english_subtitle: false,
        summary: Some("一只猫咪".to_string()),
        recommended_caption: None,
        asr_text: None,
        asr_language: None,
        frame_paths: vec![],
        model_name: Some("qwen2.5-vl:7b".to_string()),
        prompt_version: Some("v1".to_string()),
    })
    .expect("analysis");

    let ready = assets.list(AssetFilter { status: Some(AssetStatus::Ready), ..Default::default() }).expect("list");
    assert_eq!(ready.len(), 1);
    let cats = assets.list(AssetFilter { category: Some("宠物".to_string()), ..Default::default() }).expect("list");
    assert_eq!(cats.len(), 1);
    let none = assets.list(AssetFilter { category: Some("美食".to_string()), ..Default::default() }).expect("list");
    assert!(none.is_empty());
}

#[test]
fn asset_delete_cascades_analysis() {
    let db = test_db();
    let assets = AssetsRepo::new(&db);
    let analyses = AnalysesRepo::new(&db);
    let id = assets.insert(sample_asset("del")).expect("insert");
    analyses
        .upsert(NewAnalysis {
            asset_id: id,
            category: None,
            tags: vec![],
            quality_score: None,
            has_foreign_language: false,
            has_large_amount_english_subtitle: false,
            summary: None,
            recommended_caption: None,
            asr_text: None,
            asr_language: None,
            frame_paths: vec![],
            model_name: None,
            prompt_version: None,
        })
        .expect("analysis");
    assets.delete(id).expect("delete");
    assert!(assets.get(id).expect("get").is_none());
    assert!(analyses.get_by_asset(id).expect("get").is_none());
}

#[test]
fn analysis_upsert_updates_existing_row() {
    let db = test_db();
    let assets = AssetsRepo::new(&db);
    let analyses = AnalysesRepo::new(&db);
    let id = assets.insert(sample_asset("upd")).expect("insert");
    let base = NewAnalysis {
        asset_id: id,
        category: Some("宠物".to_string()),
        tags: vec![],
        quality_score: Some(80),
        has_foreign_language: false,
        has_large_amount_english_subtitle: false,
        summary: None,
        recommended_caption: None,
        asr_text: None,
        asr_language: None,
        frame_paths: vec![],
        model_name: None,
        prompt_version: None,
    };
    analyses.upsert(base.clone()).expect("first");
    let second = NewAnalysis {
        quality_score: Some(95),
        ..base
    };
    analyses.upsert(second).expect("second");
    let row = analyses.get_by_asset(id).expect("get").expect("some");
    assert_eq!(row.quality_score, Some(95));
}

#[test]
fn task_enqueue_claim_and_succeed() {
    let db = test_db();
    let repo = TasksRepo::new(&db);
    let id = repo.enqueue("vision", "analyze_asset", serde_json::json!({"asset_id": 1}), false).expect("enqueue");
    let task = repo.claim_next("vision").expect("claim").expect("some");
    assert_eq!(task.id, id);
    assert_eq!(task.status, TaskStatus::Running);
    assert_eq!(task.attempts, 1);
    repo.succeed(id).expect("succeed");
    assert_eq!(repo.get(id).expect("get").expect("some").status, TaskStatus::Succeeded);
    assert!(repo.claim_next("vision").expect("claim").is_none());
}

#[test]
fn task_claim_respects_single_worker_per_agent() {
    let db = test_db();
    let repo = TasksRepo::new(&db);
    repo.enqueue("vision", "a", serde_json::json!({}), false).expect("1");
    repo.enqueue("vision", "b", serde_json::json!({}), false).expect("2");
    let first = repo.claim_next("vision").expect("claim").expect("first");
    assert!(repo.claim_next("vision").expect("claim").is_none(), "running task blocks queue");
    repo.succeed(first.id).expect("succeed");
    assert!(repo.claim_next("vision").expect("claim").is_some(), "next task claimable after finish");
}

#[test]
fn task_failure_schedules_retry_then_final_failure() {
    let db = test_db();
    let repo = TasksRepo::new(&db);
    let id = repo.enqueue("vision", "t", serde_json::json!({}), false).expect("enqueue");
    repo.claim_next("vision").expect("claim").expect("some");
    repo.fail(id, "boom").expect("fail 1");
    let t = repo.get(id).expect("get").expect("some");
    assert_eq!(t.status, TaskStatus::Pending);
    assert_eq!(t.attempts, 1);
    assert!(t.next_retry_at.is_some(), "retry time must be scheduled");
    // 退避未到期时不可领取
    assert!(repo.claim_next("vision").expect("claim").is_none(), "backoff blocks early claim");
    // 模拟退避到期：将重试时间置为过去
    db.conn()
        .execute(
            "UPDATE agent_tasks SET next_retry_at = '2000-01-01T00:00:00Z' WHERE id = ?1",
            rusqlite::params![id],
        )
        .expect("clear retry");
    repo.claim_next("vision").expect("claim").expect("retry claim");
    repo.fail(id, "boom again").expect("fail 2");
    let t = repo.get(id).expect("get").expect("some");
    assert_eq!(t.attempts, 2);
    // 第三次失败达到 max_attempts=3
    db.conn()
        .execute(
            "UPDATE agent_tasks SET next_retry_at = '2000-01-01T00:00:00Z' WHERE id = ?1",
            rusqlite::params![id],
        )
        .expect("clear retry");
    repo.claim_next("vision").expect("claim").expect("last claim");
    repo.fail(id, "boom final").expect("fail 3");
    let t = repo.get(id).expect("get").expect("some");
    assert_eq!(t.status, TaskStatus::Failed);
    assert!(repo.claim_next("vision").expect("claim").is_none());
}

#[test]
fn task_human_gate_wait_resume_cancel() {
    let db = test_db();
    let repo = TasksRepo::new(&db);
    let id = repo.enqueue("publisher", "prepare_publish", serde_json::json!({}), true).expect("enqueue");
    repo.claim_next("publisher").expect("claim").expect("some");
    repo.wait_for_human(id).expect("wait");
    assert_eq!(repo.get(id).expect("get").expect("some").status, TaskStatus::WaitingForHuman);
    // 等待人工时不可被其他 worker 领取
    assert!(repo.claim_next("publisher").expect("claim").is_none());
    repo.resume(id).expect("resume");
    repo.succeed(id).expect("succeed");
    let id2 = repo.enqueue("publisher", "p2", serde_json::json!({}), true).expect("enqueue");
    repo.claim_next("publisher").expect("claim").expect("some");
    repo.wait_for_human(id2).expect("wait");
    repo.cancel(id2).expect("cancel");
    assert_eq!(repo.get(id2).expect("get").expect("some").status, TaskStatus::Cancelled);
}
