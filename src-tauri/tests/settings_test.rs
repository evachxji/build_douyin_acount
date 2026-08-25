use std::collections::HashMap;

use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::settings::{AppSettings, SettingsRepo};

fn test_db() -> Database {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("test.db");
    Database::open(&path).expect("open db")
}

#[test]
fn settings_defaults_when_nothing_stored() {
    let db = test_db();
    let settings = SettingsRepo::new(&db).load().expect("load");
    assert_eq!(settings.vision_model, "qwen2.5vl:7b");
    assert_eq!(settings.llm_model, "qwen2.5:7b");
    assert_eq!(settings.whisper_model, "base");
    assert_eq!(settings.quality_min_keep, 60);
    assert!(settings.hit_conditions.is_empty());
    assert!(settings.exclude_conditions.is_empty());
    assert_eq!(settings.node_bin, "node");
    assert_eq!(settings.publisher_browser, "chrome");
    assert_eq!(settings.collector_script, "scripts/collector/collect.js");
    assert_eq!(settings.ytdlp_path, "yt-dlp");
    assert_eq!(settings.account_style, "治愈");
    assert_eq!(settings.batch_size, 20);
    assert_eq!(settings.tiktok_ratio, 0.3);
    assert_eq!(settings.instagram_ratio, 0.7);
    assert_eq!(settings.max_likes, 1_000_000);
}

#[test]
fn settings_roundtrip_persists_fields() {
    let db = test_db();
    let repo = SettingsRepo::new(&db);
    let custom = AppSettings {
        ollama_url: "http://127.0.0.1:11434".to_string(),
        vision_model: "qwen2.5vl:14b".to_string(),
        llm_model: "qwen3:8b".to_string(),
        whisper_python: "py".to_string(),
        whisper_script: "ws.py".to_string(),
        whisper_model: "small".to_string(),
        quality_min_keep: 70,
        node_bin: "node-dev".to_string(),
        publisher_script: "scripts/dev-publish.js".to_string(),
        publisher_browser: "chromium".to_string(),
        collector_script: "scripts/dev-collect.js".to_string(),
        ytdlp_path: "yt-dlp-dev".to_string(),
        douyin_sync_script: "scripts/dev-sync.js".to_string(),
        account_style: "搞笑".to_string(),
        batch_size: 30,
        tiktok_ratio: 0.4,
        instagram_ratio: 0.6,
        max_likes: 500_000,
        hit_conditions: vec!["宠物".to_string(), "风景".to_string()],
        exclude_conditions: vec!["色情".to_string()],
        condition_basis: HashMap::new(),
    };
    repo.save(&custom).expect("save");

    let loaded = SettingsRepo::new(&db).load().expect("reload");
    assert_eq!(loaded, custom, "saved settings must roundtrip");
}

#[test]
fn auto_sync_moderate_flag_roundtrip() {
    let db = test_db();
    let repo = SettingsRepo::new(&db);
    assert!(!repo.get_auto_sync_moderate().expect("default"), "默认关");
    repo.set_auto_sync_moderate(true).expect("set on");
    assert!(repo.get_auto_sync_moderate().expect("get on"));
    repo.set_auto_sync_moderate(false).expect("set off");
    assert!(!repo.get_auto_sync_moderate().expect("get off"));
}

#[test]
fn auto_sync_moderate_flag_independent_of_app_settings_json() {
    // 开关是独立 key：保存完整 AppSettings 不得覆盖它，反之亦然。
    let db = test_db();
    let repo = SettingsRepo::new(&db);
    repo.set_auto_sync_moderate(true).expect("set on");
    repo.save(&AppSettings::default()).expect("save app settings");
    assert!(repo.get_auto_sync_moderate().expect("still on"), "保存 AppSettings 不得重置开关");
    repo.set_auto_sync_moderate(false).expect("set off");
    let loaded = repo.load().expect("load app settings");
    assert_eq!(loaded, AppSettings::default(), "开关读写不得触碰 AppSettings JSON");
}

#[test]
fn settings_load_legacy_json_without_conditions_keeps_defaults() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(&dir.path().join("test.db")).expect("open db");
    let legacy = r#"{"ollama_url":"http://localhost:11434","vision_model":"qwen2.5vl:7b","llm_model":"qwen2.5:7b","whisper_python":"p","whisper_script":"s","whisper_model":"base","english_ratio_threshold":0.6,"quality_min_keep":60,"node_bin":"node","publisher_script":"ps","publisher_browser":"chrome","collector_script":"cs","ytdlp_path":"yt-dlp","account_style":"治愈","batch_size":20,"tiktok_ratio":0.3,"instagram_ratio":0.7,"max_likes":1000000}"#;
    db.conn()
        .execute("INSERT INTO app_settings (key, value) VALUES ('app', ?1)", [legacy])
        .expect("insert legacy settings");
    let loaded = SettingsRepo::new(&db).load().expect("load");
    assert_eq!(loaded.quality_min_keep, 60, "existing field kept");
    assert!(loaded.hit_conditions.is_empty(), "missing field defaults to empty");
    assert!(loaded.exclude_conditions.is_empty(), "missing field defaults to empty");
    assert!(loaded.condition_basis.is_empty(), "missing field defaults to empty");
    assert_eq!(
        loaded.douyin_sync_script, "scripts/douyin-sync/sync.js",
        "missing douyin_sync_script falls back to default instead of resetting all settings"
    );
}
