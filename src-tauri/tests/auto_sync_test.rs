//! 自动巡检调度测试：开关 / 30 分钟间隔 / 活跃任务防重。
use build_douyin_acount_lib::auto_sync;
use build_douyin_acount_lib::db::repos::tasks::TasksRepo;
use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::settings::SettingsRepo;
use serde_json::json;

fn test_db() -> Database {
    let dir = tempfile::tempdir().expect("tempdir");
    Database::open(&dir.path().join("test.db")).expect("open db")
}

fn enable(db: &Database) {
    SettingsRepo::new(db)
        .set_auto_sync_moderate(true)
        .expect("enable flag");
}

fn sync_task_count(db: &Database) -> usize {
    TasksRepo::new(db)
        .list(None)
        .expect("list")
        .into_iter()
        .filter(|t| t.agent_name == "douyin_sync")
        .count()
}

#[test]
fn auto_sync_enqueues_nothing_when_flag_off() {
    let db = test_db();
    assert!(!auto_sync::maybe_enqueue(&db).expect("run"));
    assert_eq!(sync_task_count(&db), 0, "开关关闭不得入队");
}

#[test]
fn auto_sync_enqueues_with_auto_source_when_flag_on() {
    let db = test_db();
    enable(&db);
    assert!(auto_sync::maybe_enqueue(&db).expect("run"));
    let tasks = TasksRepo::new(&db).list(None).expect("list");
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].agent_name, "douyin_sync");
    assert_eq!(
        tasks[0].payload["source"], "auto",
        "payload 须带 source=auto 供完成分支联动违规处理"
    );
}

#[test]
fn auto_sync_skips_when_sync_already_active() {
    let db = test_db();
    enable(&db);
    assert!(auto_sync::maybe_enqueue(&db).expect("first run"));
    // 第一轮入队的任务还在排队（PENDING）：不得重复入队。
    assert!(!auto_sync::maybe_enqueue(&db).expect("second run"));
    assert_eq!(sync_task_count(&db), 1);
}

#[test]
fn auto_sync_skips_when_moderate_active() {
    // moderate 完成后会自行入队一轮刷新 sync，调度器不得再添一轮。
    let db = test_db();
    enable(&db);
    TasksRepo::new(&db)
        .enqueue("douyin_moderate", "moderate_violation", json!({"mode": "all"}), false)
        .expect("seed moderate");
    assert!(!auto_sync::maybe_enqueue(&db).expect("run"));
    assert_eq!(sync_task_count(&db), 0);
}

#[test]
fn auto_sync_skips_when_recent_sync_within_interval() {
    // 25 分钟前已完成一轮：未到 30 分钟间隔，不发起。
    let db = test_db();
    enable(&db);
    let id = TasksRepo::new(&db)
        .enqueue("douyin_sync", "sync_works", json!({}), false)
        .expect("seed sync");
    db.conn()
        .execute(
            "UPDATE agent_tasks SET status = 'SUCCEEDED',
                 created_at = datetime('now', '-25 minutes') WHERE id = ?1",
            [id],
        )
        .expect("backdate");
    assert!(!auto_sync::maybe_enqueue(&db).expect("run"));
    assert_eq!(sync_task_count(&db), 1, "不得新增");
}

#[test]
fn auto_sync_enqueues_after_interval_elapsed() {
    // 31 分钟前的一轮（手动发起、无 source）：已超过间隔，发起新一轮。
    let db = test_db();
    enable(&db);
    let id = TasksRepo::new(&db)
        .enqueue("douyin_sync", "sync_works", json!({}), false)
        .expect("seed sync");
    db.conn()
        .execute(
            "UPDATE agent_tasks SET status = 'SUCCEEDED',
                 created_at = datetime('now', '-31 minutes') WHERE id = ?1",
            [id],
        )
        .expect("backdate");
    assert!(auto_sync::maybe_enqueue(&db).expect("run"));
    assert_eq!(sync_task_count(&db), 2);
}
