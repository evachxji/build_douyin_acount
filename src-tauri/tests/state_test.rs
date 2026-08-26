use std::path::PathBuf;

use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::media::MediaStore;
use build_douyin_acount_lib::settings::{AppSettings, SettingsRepo};
use build_douyin_acount_lib::state::AppState;

fn temp_db() -> Database {
    let dir = tempfile::tempdir().expect("tempdir");
    Database::open(&dir.path().join("test.db")).expect("open db")
}

#[test]
fn app_state_builds_runtime_with_vision_agent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = temp_db();
    let settings = AppSettings {
        ollama_url: "http://127.0.0.1:11434".to_string(),
        vision_model: "qwen2.5vl:7b".to_string(),
        ..AppSettings::default()
    };
    SettingsRepo::new(&db).save(&settings).expect("save");
    let state = AppState::new(
        db,
        MediaStore::new(dir.path().join("storage")),
        settings,
    );
    let runtime = state.build_runtime().expect("build runtime");
    assert!(runtime.agent_names().contains(&"vision"));
    let _ = PathBuf::new();
}

#[test]
fn build_pipeline_with_openai_provider_requires_api_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let settings = AppSettings {
        vision_provider: "openai".to_string(),
        openai_vision_api_key: String::new(),
        ..AppSettings::default()
    };
    let state = AppState::new(temp_db(), MediaStore::new(dir.path().join("storage")), settings);
    let err = match state.build_pipeline() {
        Ok(_) => panic!("empty key must fail fast"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("API Key"), "got: {err}");
}

#[test]
fn build_pipeline_with_openai_providers_builds_when_key_present() {
    let dir = tempfile::tempdir().expect("tempdir");
    let settings = AppSettings {
        vision_provider: "openai".to_string(),
        llm_provider: "openai".to_string(),
        openai_vision_api_key: "sk-test".to_string(),
        openai_llm_api_key: "sk-test".to_string(),
        ..AppSettings::default()
    };
    let state = AppState::new(temp_db(), MediaStore::new(dir.path().join("storage")), settings);
    state.build_pipeline().expect("openai providers with key must build");
}

#[test]
fn build_pipeline_supports_mixed_providers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let settings = AppSettings {
        vision_provider: "ollama".to_string(),
        llm_provider: "openai".to_string(),
        openai_llm_api_key: "sk-test".to_string(),
        ..AppSettings::default()
    };
    let state = AppState::new(temp_db(), MediaStore::new(dir.path().join("storage")), settings);
    state
        .build_pipeline()
        .expect("mixed providers (local vision + cloud llm) must build");
}
