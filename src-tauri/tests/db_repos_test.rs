use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::db::repos::assets::{AssetFilter, AssetsRepo};
use build_douyin_acount_lib::db::repos::analyses::AnalysesRepo;
use build_douyin_acount_lib::db::repos::tasks::TasksRepo;
use build_douyin_acount_lib::domain::analysis::{ConditionVerdict, NewAnalysis};
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
fn legacy_db_gets_condition_results_column() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("legacy.db");
    {
        let conn = rusqlite::Connection::open(&path).expect("open");
        conn.execute_batch(
            "CREATE TABLE analysis_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                asset_id INTEGER NOT NULL UNIQUE,
                category TEXT,
                tags_json TEXT NOT NULL DEFAULT '[]',
                quality_score INTEGER,
                has_foreign_language INTEGER NOT NULL DEFAULT 0,
                has_large_amount_english_subtitle INTEGER NOT NULL DEFAULT 0,
                summary TEXT,
                recommended_caption TEXT,
                asr_text TEXT,
                asr_language TEXT,
                frame_paths_json TEXT NOT NULL DEFAULT '[]',
                model_name TEXT,
                prompt_version TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .expect("create legacy table");
    }
    let db = Database::open(&path).expect("open upgrades legacy db");
    let cols: Vec<String> = db
        .conn()
        .prepare("PRAGMA table_info(analysis_results)")
        .expect("pragma")
        .query_map([], |r| r.get::<_, String>(1))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("collect");
    assert!(cols.iter().any(|c| c == "condition_results_json"));
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
    repo.update_status(id, AssetStatus::Candidate).expect("ok");
}


#[test]
fn asset_list_filters_by_status_and_category() {
    let db = test_db();
    let assets = AssetsRepo::new(&db);
    let analyses = AnalysesRepo::new(&db);
    let id = assets.insert(sample_asset("cat_video")).expect("insert");
    assets.update_status(id, AssetStatus::Analyzing).expect("analyzing");
    assets.update_status(id, AssetStatus::Candidate).expect("CANDIDATE");
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
        condition_results: vec![],
    })
    .expect("analysis");

    let ready = assets.list(AssetFilter { status: Some(AssetStatus::Candidate), ..Default::default() }).expect("list");
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
            condition_results: vec![],
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
        condition_results: vec![],
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
fn analysis_condition_results_roundtrip() {
    let db = test_db();
    let assets = AssetsRepo::new(&db);
    let analyses = AnalysesRepo::new(&db);
    let id = assets.insert(sample_asset("cond")).expect("insert");
    let verdicts = vec![
        ConditionVerdict { name: "宠物".to_string(), hit: true, basis: "画面".to_string() },
        ConditionVerdict { name: "长中文对话".to_string(), hit: false, basis: "音频".to_string() },
    ];
    analyses
        .upsert(NewAnalysis {
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
            condition_results: verdicts.clone(),
        })
        .expect("upsert");
    let row = analyses.get_by_asset(id).expect("get").expect("some");
    assert_eq!(row.condition_results, verdicts);
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
    // RUNNING 期间重新领取返回同一任务（恢复语义），不会跳到 PENDING 的 b。
    let again = repo.claim_next("vision").expect("claim").expect("reclaim");
    assert_eq!(again.id, first.id, "running task is re-claimed, not the queued b");
    assert_eq!(again.attempts, first.attempts, "re-claim must not increment attempts");
    repo.succeed(first.id).expect("succeed");
    let next = repo.claim_next("vision").expect("claim").expect("next task");
    assert_eq!(next.id, 2, "after finish, queued task b is claimable");
}

#[test]
fn task_resume_after_waiting_for_human_is_reclaimed() {
    let db = test_db();
    let repo = TasksRepo::new(&db);
    let id = repo.enqueue("publisher", "prepare_publish", serde_json::json!({}), true).expect("enqueue");
    repo.claim_next("publisher").expect("claim").expect("some");
    repo.wait_for_human(id).expect("wait");
    // WAITING_FOR_HUMAN 阻塞队列
    assert!(repo.claim_next("publisher").expect("claim").is_none());
    // 人工放行 -> RUNNING -> worker 重新领取同一任务继续执行
    repo.resume(id).expect("resume");
    let again = repo.claim_next("publisher").expect("claim").expect("reclaim");
    assert_eq!(again.id, id);
    assert_eq!(again.attempts, 1, "resume must not count as a retry");
}

#[test]
fn task_left_running_by_crash_is_recovered() {
    let db = test_db();
    let repo = TasksRepo::new(&db);
    let id = repo.enqueue("publisher", "prepare_publish", serde_json::json!({}), false).expect("enqueue");
    repo.claim_next("publisher").expect("claim").expect("some");
    // 模拟进程在 RUNNING 中途退出：不做任何收尾，直接重启后重新领取
    let recovered = repo.claim_next("publisher").expect("claim").expect("recovered");
    assert_eq!(recovered.id, id);
    assert_eq!(recovered.status, TaskStatus::Running);
    assert_eq!(recovered.attempts, 1);
}

#[test]
fn task_wait_for_human_persists_context_in_payload() {
    let db = test_db();
    let repo = TasksRepo::new(&db);
    let id = repo
        .enqueue(
            "publisher",
            "prepare_publish",
            serde_json::json!({ "draft_id": 4, "asset_id": 2 }),
            true,
        )
        .expect("enqueue");
    repo.claim_next("publisher").expect("claim").expect("some");
    repo.wait_for_human_with_context(
        id,
        serde_json::json!({ "stage": "file", "path": "videos/x.mp4", "message": "hi" }),
    )
    .expect("wait with context");
    let t = repo.get(id).expect("get").expect("some");
    assert_eq!(t.status, TaskStatus::WaitingForHuman);
    // 执行字段保留，上下文字段可读
    assert_eq!(t.payload["draft_id"], 4);
    assert_eq!(t.payload["asset_id"], 2);
    assert_eq!(t.payload["stage"], "file");
    assert_eq!(t.payload["path"], "videos/x.mp4");
    assert_eq!(t.payload["message"], "hi");
}

#[test]
fn task_update_payload_merges_stage_info_keeping_execution_fields() {
    let db = test_db();
    let repo = TasksRepo::new(&db);
    let id = repo
        .enqueue(
            "publisher",
            "prepare_publish",
            serde_json::json!({ "draft_id": 4, "asset_id": 2 }),
            true,
        )
        .expect("enqueue");
    repo.claim_next("publisher").expect("claim").expect("some");
    repo.update_payload(
        id,
        serde_json::json!({ "stage": "file", "title": "标题", "path": "videos/x.mp4" }),
    )
    .expect("update payload");
    let t = repo.get(id).expect("get").expect("some");
    assert_eq!(t.payload["draft_id"], 4);
    assert_eq!(t.payload["asset_id"], 2);
    assert_eq!(t.payload["stage"], "file");
    assert_eq!(t.payload["title"], "标题");
    assert_eq!(t.payload["path"], "videos/x.mp4");
}

#[test]
fn task_running_can_be_cancelled_by_human() {
    let db = test_db();
    let repo = TasksRepo::new(&db);
    let id = repo
        .enqueue("publisher", "prepare_publish", serde_json::json!({}), true)
        .expect("enqueue");
    repo.claim_next("publisher").expect("claim").expect("some");
    repo.cancel(id).expect("cancel running task");
    let t = repo.get(id).expect("get").expect("some");
    assert_eq!(t.status, TaskStatus::Cancelled);
}

#[test]
fn draft_repo_create_list_and_status_flow() {
    use build_douyin_acount_lib::db::repos::drafts::DraftsRepo;
    use build_douyin_acount_lib::domain::draft::{DraftStatus, NewDraft};

    let db = test_db();
    let repo = DraftsRepo::new(&db);
    // publish_drafts.asset_id 外键指向 video_assets，先建一个素材。
    let asset_id = AssetsRepo::new(&db)
        .insert(NewAsset {
            dedup_key: Some("draft-test".to_string()),
            file_path: "videos/draft.mp4".to_string(),
            cover_path: None,
            source_platform: "instagram".to_string(),
            source_url: None,
            author: None,
            title: None,
            description: None,
            published_at: None,
            tags: Vec::new(),
            likes: 0,
            comments: 0,
            plays: 0,
            duration_ms: 1000,
        })
        .expect("insert asset");
    let id = repo
        .create(NewDraft {
            asset_id,
            style_id: None,
            title: Some("标题".to_string()),
            body: Some("正文".to_string()),
            hashtags: vec!["萌宠".to_string(), "猫".to_string()],
        })
        .expect("create");
    let draft = repo.get(id).expect("get").expect("some");
    assert_eq!(draft.status, DraftStatus::Draft);
    assert_eq!(draft.hashtags, vec!["萌宠", "猫"]);

    assert_eq!(repo.list().expect("list").len(), 1);
    repo.update_status(id, DraftStatus::Filled).expect("filled");
    repo.update_status(id, DraftStatus::Published).expect("published");
    let published = repo.get(id).expect("get").expect("some");
    assert_eq!(published.status, DraftStatus::Published);

    // 非法迁移：PUBLISHED 不能再回 FILLED
    let err = repo.update_status(id, DraftStatus::Filled).expect_err("illegal");
    assert!(err.to_string().contains("illegal status transition"));
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
