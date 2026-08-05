use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::db::repos::tasks::TasksRepo;
use build_douyin_acount_lib::domain::task::TaskStatus;

fn test_db() -> Database {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("test.db");
    Database::open(&path).expect("open db")
}

#[test]
fn task_list_returns_all_and_filters_by_status() {
    let db = test_db();
    let repo = TasksRepo::new(&db);
    let pending = repo
        .enqueue("vision", "analyze", serde_json::json!({"asset_id": 1}), false)
        .expect("enqueue");
    let human = repo
        .enqueue("publisher", "prepare_publish", serde_json::json!({}), true)
        .expect("enqueue");
    repo.claim_next("publisher").expect("claim").expect("some");
    repo.wait_for_human(human).expect("wait");

    let all = repo.list(None).expect("list all");
    assert_eq!(all.len(), 2);

    let waiting = repo
        .list(Some(TaskStatus::WaitingForHuman))
        .expect("list waiting");
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].id, human);

    let pending_only = repo.list(Some(TaskStatus::Pending)).expect("list pending");
    assert_eq!(pending_only.len(), 1);
    assert_eq!(pending_only[0].id, pending);
}
