use build_douyin_acount_lib::commands::analysis::enqueue_analysis;
use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::db::repos::assets::AssetsRepo;
use build_douyin_acount_lib::db::repos::tasks::TasksRepo;
use build_douyin_acount_lib::domain::asset::NewAsset;

fn test_db() -> Database {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("test.db");
    Database::open(&path).expect("open db")
}

fn seed_assets(db: &Database, n: i64) -> Vec<i64> {
    let repo = AssetsRepo::new(db);
    (1..=n)
        .map(|i| {
            repo.insert(NewAsset {
                dedup_key: Some(format!("seed:{i}")),
                file_path: format!("videos/{i}.mp4"),
                cover_path: None,
                source_platform: "local".to_string(),
                source_url: None,
                author: None,
                title: Some(format!("v{i}")),
                description: None,
                published_at: None,
                tags: Vec::new(),
                likes: 0,
                comments: 0,
                plays: 0,
                duration_ms: 5000,
            })
            .expect("insert")
        })
        .collect()
}

#[test]
fn enqueue_analysis_skips_assets_with_active_tasks() {
    let db = test_db();
    let ids = seed_assets(&db, 4);

    let first = enqueue_analysis(&db, &ids[..3]).expect("enqueue first batch");
    assert_eq!(first, ids[..3].to_vec());

    // 已有 PENDING 任务的资产不能重复入队
    let second = enqueue_analysis(&db, &ids[1..4]).expect("enqueue second batch");
    assert_eq!(second, vec![ids[3]], "only the new asset should be enqueued");

    // 任务结束后（SUCCEEDED）允许重新分析
    let task = TasksRepo::new(&db)
        .claim_next("vision")
        .expect("claim")
        .expect("task");
    TasksRepo::new(&db).succeed(task.id).expect("succeed");
    let third = enqueue_analysis(&db, &[ids[0]]).expect("enqueue third");
    assert_eq!(third, vec![ids[0]], "finished asset can be re-analyzed");
}
