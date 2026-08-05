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
