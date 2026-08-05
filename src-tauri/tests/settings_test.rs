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
    assert_eq!(settings.english_ratio_threshold, 0.6);
    assert_eq!(settings.quality_min_keep, 60);
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
        english_ratio_threshold: 0.7,
        quality_min_keep: 70,
    };
    repo.save(&custom).expect("save");

    let loaded = SettingsRepo::new(&db).load().expect("reload");
    assert_eq!(loaded, custom, "saved settings must roundtrip");
}
