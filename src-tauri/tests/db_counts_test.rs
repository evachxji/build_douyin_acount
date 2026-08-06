use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::db::repos::analyses::AnalysesRepo;
use build_douyin_acount_lib::db::repos::assets::AssetsRepo;
use build_douyin_acount_lib::domain::analysis::NewAnalysis;
use build_douyin_acount_lib::domain::asset::{AssetStatus, NewAsset};

fn test_db() -> Database {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("test.db");
    Database::open(&path).expect("open db")
}

fn sample_asset(title: &str) -> NewAsset {
    NewAsset {
        dedup_key: Some(format!("ig:{title}")),
        file_path: format!("videos/{title}.mp4"),
        cover_path: None,
        source_platform: "instagram".to_string(),
        source_url: None,
        author: Some("a".to_string()),
        title: Some(title.to_string()),
        description: None,
        published_at: None,
        tags: Vec::new(),
        likes: 1,
        comments: 0,
        plays: 10,
        duration_ms: 3000,
    }
}

#[test]
fn category_counts_groups_by_analysis_category() {
    let db = test_db();
    let assets = AssetsRepo::new(&db);
    let analyses = AnalysesRepo::new(&db);

    let pet = assets.insert(sample_asset("cat")).expect("insert");
    let food = assets.insert(sample_asset("cake")).expect("insert");
    assets.insert(sample_asset("misc")).expect("insert");

    let mut analysis = NewAnalysis {
        asset_id: pet,
        category: Some("宠物".to_string()),
        tags: vec!["猫".to_string()],
        quality_score: Some(80),
        has_foreign_language: false,
        has_large_amount_english_subtitle: false,
        summary: Some("s".to_string()),
        recommended_caption: Some("c".to_string()),
        asr_text: None,
        asr_language: None,
        frame_paths: Vec::new(),
        model_name: None,
        prompt_version: None,
        condition_results: vec![],
    };
    analyses.upsert(analysis.clone()).expect("upsert pet");
    analysis.asset_id = food;
    analysis.category = Some("美食".to_string());
    analyses.upsert(analysis).expect("upsert food");

    let counts = assets.category_counts().expect("counts");
    let map: std::collections::HashMap<String, i64> = counts.into_iter().collect();
    assert_eq!(map.get("宠物"), Some(&1));
    assert_eq!(map.get("美食"), Some(&1));
    assert_eq!(map.get("未分类"), Some(&1), "asset without analysis counts as 未分类");
}

#[test]
fn status_counts_match_inserted_rows() {
    let db = test_db();
    let repo = AssetsRepo::new(&db);
    let a = repo.insert(sample_asset("a")).expect("insert a");
    let b = repo.insert(sample_asset("b")).expect("insert b");
    repo.insert(sample_asset("c")).expect("insert c");
    repo.update_status(a, AssetStatus::Analyzing).expect("analyzing");
    repo.update_status(a, AssetStatus::Candidate).expect("CANDIDATE");
    repo.update_status(b, AssetStatus::Analyzing).expect("analyzing b");
    repo.update_status(b, AssetStatus::Candidate).expect("ready b");
    repo.update_status(b, AssetStatus::Selected).expect("selected");

    let counts = repo.status_counts().expect("counts");
    let map: std::collections::HashMap<String, i64> = counts.into_iter().collect();
    assert_eq!(map.get("NEW"), Some(&1));
    assert_eq!(map.get("CANDIDATE"), Some(&1));
    assert_eq!(map.get("SELECTED"), Some(&1));
}
