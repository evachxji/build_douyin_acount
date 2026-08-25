//! Publisher Agent 集成测试：用假 sidecar 验证「自动上传文件 -> 自动填表 ->
//! 人工确认发布」的完整人机协作状态流。
use std::path::{Path, PathBuf};

use build_douyin_acount_lib::agents::publisher::{
    pick_schedule_time, request_confirm, PublisherAgent,
};
use build_douyin_acount_lib::agents::{Agent, AgentContext, AgentOutcome};
use build_douyin_acount_lib::db::repos::assets::AssetsRepo;
use build_douyin_acount_lib::db::repos::douyin_posts::DouyinPostsRepo;
use build_douyin_acount_lib::db::repos::drafts::DraftsRepo;
use build_douyin_acount_lib::db::repos::tasks::TasksRepo;
use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::domain::asset::{AssetStatus, NewAsset};
use build_douyin_acount_lib::domain::douyin::NewDouyinPost;
use build_douyin_acount_lib::domain::draft::{DraftStatus, NewDraft};
use build_douyin_acount_lib::domain::task::TaskStatus;
use build_douyin_acount_lib::media::MediaStore;
use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use serde_json::json;
use std::collections::HashMap;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

struct TestEnv {
    _dir: tempfile::TempDir,
    db: Database,
    store: MediaStore,
    agent: PublisherAgent,
    asset_id: i64,
    draft_id: i64,
    task_id: i64,
}

fn setup() -> TestEnv {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = Database::open(&dir.path().join("test.db")).expect("db");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("dirs");

    let assets = AssetsRepo::new(&db);
    let asset_id = assets
        .insert(NewAsset {
            dedup_key: Some("publisher-test".to_string()),
            file_path: "videos/test.mp4".to_string(),
            cover_path: None,
            source_platform: "instagram".to_string(),
            source_url: None,
            author: Some("tester".to_string()),
            title: Some("测试视频".to_string()),
            description: None,
            published_at: None,
            tags: vec!["猫".to_string()],
            likes: 0,
            comments: 0,
            plays: 0,
            duration_ms: 5000,
        })
        .expect("insert asset");
    assets
        .update_status(asset_id, AssetStatus::Analyzing)
        .expect("analyzing");
    assets
        .update_status(asset_id, AssetStatus::Candidate)
        .expect("CANDIDATE");
    assets
        .update_status(asset_id, AssetStatus::Selected)
        .expect("selected");

    let draft_id = DraftsRepo::new(&db)
        .create(NewDraft {
            asset_id,
            style_id: None,
            title: Some("测试标题".to_string()),
            body: Some("测试文案".to_string()),
            hashtags: vec!["萌宠".to_string()],
        })
        .expect("create draft");

    let task_id = TasksRepo::new(&db)
        .enqueue(
            "publisher",
            "prepare_publish",
            json!({ "draft_id": draft_id, "asset_id": asset_id }),
            true,
        )
        .expect("enqueue");
    let agent = PublisherAgent::new("node", fixture("fake_publish.cjs"));
    TestEnv {
        _dir: dir,
        db,
        store,
        agent,
        asset_id,
        draft_id,
        task_id,
    }
}

/// 领取任务并返回 RUNNING 状态的任务（模拟 worker run_once）。
fn claim(env: &TestEnv) -> build_douyin_acount_lib::domain::task::AgentTask {
    TasksRepo::new(&env.db)
        .claim_next("publisher")
        .expect("claim")
        .expect("some task")
}

fn progress_path(env: &TestEnv) -> PathBuf {
    env.store
        .root()
        .join("publish")
        .join("progress")
        .join(format!("{}.json", env.draft_id))
}

fn write_progress(env: &TestEnv, state: &str, message: &str) {
    let path = progress_path(env);
    std::fs::create_dir_all(path.parent().unwrap()).expect("dirs");
    std::fs::write(
        path,
        serde_json::to_string(&json!({ "state": state, "message": message })).unwrap(),
    )
    .expect("write progress");
}

/// 带发布捕获信息的 published 进度（模拟 sidecar 钩子捕获到页面实际值）。
fn write_published_with_post(env: &TestEnv, post: serde_json::Value) {
    let path = progress_path(env);
    std::fs::create_dir_all(path.parent().unwrap()).expect("dirs");
    std::fs::write(
        path,
        serde_json::to_string(&json!({
            "state": "published",
            "message": "页面已跳转到内容管理，发布已提交",
            "post": post,
        }))
        .unwrap(),
    )
    .expect("write progress");
}

/// "YYYY-MM-DD HH:MM"（本地时间）→ epoch 秒（与被测代码同口径）。
fn local_ts(s: &str) -> i64 {
    let ndt = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").expect("parse");
    Local.from_local_datetime(&ndt).single().expect("local").timestamp()
}

#[test]
fn publisher_file_stage_keeps_task_running_with_info_payload() {
    let env = setup();
    // 预写 waiting_for_file（自动上传失败的兜底态）：验证任务保持 RUNNING 且 payload 写入阶段信息。
    write_progress(&env, "waiting_for_file", "waiting");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Continue));
    let t = TasksRepo::new(&env.db).get(env.task_id).expect("get").expect("task");
    assert_eq!(t.status, TaskStatus::Running);
    assert_eq!(t.payload["stage"], "file");
    assert_eq!(t.payload["title"], "测试标题");
    assert_eq!(t.payload["path"], "videos/test.mp4");
    // sidecar 的兜底提示消息透传到 payload，供前端弹窗展示。
    assert_eq!(t.payload["message"], "waiting");
}

#[test]
fn publisher_spawns_sidecar_and_auto_fills() {
    let env = setup();
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    // 模拟 worker 每轮重新领取：sidecar 拉起后自动完成 uploading -> form_filled，
    // agent 在后续轮次看到 form_filled 进入 form_ready。
    let mut outcome = None;
    for _ in 0..20 {
        let task = claim(&env);
        outcome = Some(env.agent.run(&ctx, task).expect("run"));
        if !matches!(outcome.as_ref().unwrap(), AgentOutcome::Continue) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    let outcome = outcome.expect("ran at least once");
    let payload = match outcome {
        AgentOutcome::WaitingForHuman(p) => p,
        other => panic!("expected WaitingForHuman, got {other:?}"),
    };
    assert_eq!(payload["stage"], "form_ready");
    let draft = DraftsRepo::new(&env.db).get(env.draft_id).expect("get").expect("draft");
    assert_eq!(draft.status, DraftStatus::Filled);
}

#[test]
fn publisher_auto_detects_form_ready_after_fill() {
    let env = setup();
    // 假 sidecar 已自动完成填表
    write_progress(&env, "form_filled", "fake form filled");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    let payload = match outcome {
        AgentOutcome::WaitingForHuman(p) => p,
        other => panic!("expected WaitingForHuman, got {other:?}"),
    };
    assert_eq!(payload["stage"], "form_ready");
    let draft = DraftsRepo::new(&env.db).get(env.draft_id).expect("get").expect("draft");
    assert_eq!(draft.status, DraftStatus::Filled);
}

#[test]
fn publisher_finalizes_when_human_confirms_publish() {
    let env = setup();
    write_progress(&env, "form_filled", "filled");
    request_confirm(&env.store, env.draft_id).expect("confirm");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Succeeded(_)));
    let draft = DraftsRepo::new(&env.db).get(env.draft_id).expect("get").expect("draft");
    assert_eq!(draft.status, DraftStatus::Published);
    let asset = AssetsRepo::new(&env.db).get(env.asset_id).expect("get").expect("asset");
    assert_eq!(asset.status, AssetStatus::Published);
    // 人工确认路径无 sidecar 捕获：兜底登记（local-draft 占位 id + draft 标题 + 立即发布），
    // 日历即时更新；不再入队 douyin_sync。
    let post = DouyinPostsRepo::new(&env.db)
        .get_by_aweme(&format!("local-draft-{}", env.draft_id))
        .expect("get post")
        .expect("发布完成后应直接登记日历");
    assert_eq!(post.title.as_deref(), Some("测试标题"));
    assert_eq!(post.status, "published");
    assert!(post.public_time > 0);
    let sync = TasksRepo::new(&env.db)
        .claim_next("douyin_sync")
        .expect("claim sync");
    assert!(sync.is_none(), "直接登记日历，不再入队 douyin_sync");
}

#[test]
fn publisher_finalizes_when_sidecar_detects_success() {
    let env = setup();
    // sidecar 钩子捕获到页面实际值（用户手改过的标题与定时时间），随 published 上报。
    write_published_with_post(
        &env,
        json!({
            "awemeId": "767123000",
            "title": "页面上的标题",
            "scheduleTime": "2100-01-02 10:30",
            "coverPath": "",
        }),
    );
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Succeeded(_)));
    let asset = AssetsRepo::new(&env.db).get(env.asset_id).expect("get").expect("asset");
    assert_eq!(asset.status, AssetStatus::Published);
    // 日历登记以捕获的页面实际值为准：真实 aweme_id + 手改标题 + 定时作品（未来时间）。
    let post = DouyinPostsRepo::new(&env.db)
        .get_by_aweme("767123000")
        .expect("get post")
        .expect("发布完成后应直接登记日历");
    assert_eq!(post.title.as_deref(), Some("页面上的标题"));
    assert_eq!(post.status, "scheduled");
    assert_eq!(post.create_time, local_ts("2100-01-02 10:30"));
    assert_eq!(post.public_time, 0);
    // 直接登记，不再入队 douyin_sync。
    let sync = TasksRepo::new(&env.db)
        .claim_next("douyin_sync")
        .expect("claim sync");
    assert!(sync.is_none(), "直接登记日历，不再入队 douyin_sync");
}

#[test]
fn publisher_registers_past_schedule_time_as_published() {
    // 定时时间已过（提交后停留太久才判定完成）按已发布登记，日历落在原定时时刻。
    let env = setup();
    write_published_with_post(
        &env,
        json!({
            "awemeId": "767456000",
            "title": "旧定时",
            "scheduleTime": "2020-01-02 10:30",
            "coverPath": "",
        }),
    );
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Succeeded(_)));
    let post = DouyinPostsRepo::new(&env.db)
        .get_by_aweme("767456000")
        .expect("get post")
        .expect("应登记日历");
    assert_eq!(post.status, "published");
    assert_eq!(post.create_time, local_ts("2020-01-02 10:30"));
    assert_eq!(post.public_time, local_ts("2020-01-02 10:30"));
}

#[test]
fn publisher_cancels_when_browser_closed_before_publish() {
    // 用户关闭标签页 = 取消（终态），不再按失败重试。
    let env = setup();
    write_progress(&env, "browser_closed", "window closed");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Cancelled(_)), "{outcome:?}");
}

#[test]
fn publisher_cancels_when_sidecar_cancelled() {
    let env = setup();
    write_progress(&env, "cancelled", "用户取消");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = env.agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Cancelled(_)), "{outcome:?}");
}

#[test]
fn publisher_surfaces_sidecar_failure() {
    let env = setup();
    write_progress(&env, "failed", "selector not found");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let err = env.agent.run(&ctx, task).expect_err("must fail");
    assert!(err.to_string().contains("selector not found"), "{err}");
}

#[test]
fn publisher_respawns_when_sidecar_heartbeat_stale() {
    // 进度冻结在 uploading（模拟 sidecar 随应用重启被杀的僵尸任务）；
    // 阈值归零 => 立即判定僵尸，应重拉 sidecar 并在 payload 计数。
    let env = setup();
    write_progress(&env, "uploading", "stuck mid-upload");
    let agent = PublisherAgent::new("node", fixture("fake_publish.cjs"))
        .with_stale_after(std::time::Duration::ZERO);
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::Continue), "{outcome:?}");
    let t = TasksRepo::new(&env.db).get(env.task_id).expect("get").expect("task");
    assert_eq!(t.payload["respawns"], 1, "僵尸判定后应重拉 sidecar 并计数");
}

#[test]
fn publisher_stale_sidecar_cancelled_after_max_respawns() {
    // 已连续重拉 2 次仍无心跳 => 取消任务（终态），提示用户重新点击【准备发布】。
    let env = setup();
    write_progress(&env, "uploading", "stuck");
    TasksRepo::new(&env.db)
        .update_payload(env.task_id, json!({ "respawns": 2 }))
        .expect("seed respawns");
    let agent = PublisherAgent::new("node", fixture("fake_publish.cjs"))
        .with_stale_after(std::time::Duration::ZERO);
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = agent.run(&ctx, task).expect("run");
    match outcome {
        AgentOutcome::Cancelled(v) => {
            assert!(v["reason"].as_str().unwrap_or("").contains("无响应"), "{v}")
        }
        other => panic!("expected Cancelled, got {other:?}"),
    }
}

#[test]
fn publisher_never_respawns_form_filled_even_when_stale() {
    // form_filled 是人工等待态（确认/取消走标记文件），sidecar 死了也不需要重拉。
    let env = setup();
    write_progress(&env, "form_filled", "filled");
    let agent = PublisherAgent::new("node", fixture("fake_publish.cjs"))
        .with_stale_after(std::time::Duration::ZERO);
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let outcome = agent.run(&ctx, task).expect("run");
    assert!(matches!(outcome, AgentOutcome::WaitingForHuman(_)), "{outcome:?}");
    let t = TasksRepo::new(&env.db).get(env.task_id).expect("get").expect("task");
    assert!(t.payload.get("respawns").is_none(), "form_filled 不得计重拉次数");
}

#[test]
fn clipboard_write_roundtrip() {
    let text = format!("video-path-test-{}", std::process::id());
    build_douyin_acount_lib::commands::publish::copy_to_clipboard(text.clone())
        .expect("copy to clipboard");
    let mut clipboard = arboard::Clipboard::new().expect("open clipboard");
    assert_eq!(clipboard.get_text().expect("read clipboard"), text);
}

// ---- 定时发布时间计算（pick_schedule_time）----

fn day(m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, d).unwrap()
}

fn at(m: u32, d: u32, h: u32, min: u32, s: u32) -> NaiveDateTime {
    day(m, d).and_hms_opt(h, min, s).unwrap()
}

/// 占用日夹具：指定日期各排 1 个 10:00 视频（第一遍场景不关心时刻）。
fn occupied(days: &[NaiveDate]) -> HashMap<NaiveDate, Vec<i64>> {
    days.iter().map(|&d| (d, vec![10 * 60])).collect()
}

/// 14 天覆盖夹具：每天排 base 时刻列表，overrides 按 offset 覆盖（空 vec = 空缺日）。
fn full_days(
    today: NaiveDate,
    base: &[i64],
    overrides: &[(i64, Vec<i64>)],
) -> HashMap<NaiveDate, Vec<i64>> {
    let mut map: HashMap<NaiveDate, Vec<i64>> = (0..14)
        .map(|i| (today + chrono::Duration::days(i), base.to_vec()))
        .collect();
    for (off, mins) in overrides {
        map.insert(today + chrono::Duration::days(*off), mins.clone());
    }
    map
}

/// 分钟数（当天 0 点起）。
fn minutes(t: NaiveDateTime) -> i64 {
    i64::from(t.num_seconds_from_midnight() / 60)
}

#[test]
fn schedule_picks_nearest_free_day_skipping_occupied() {
    // 用户场景：今天 21 号，21/22/23/25 有视频，24 没有 -> 选 24 号 08:00~19:00。
    let now = at(8, 21, 10, 0, 0);
    let occ = occupied(&[day(8, 21), day(8, 22), day(8, 23), day(8, 25)]);
    let t = pick_schedule_time(&occ, now, 0).expect("应找到空档日");
    assert_eq!(t.date(), day(8, 24));
    assert!((8 * 60..19 * 60).contains(&minutes(t)), "须在 08:00~19:00: {t}");
}

#[test]
fn schedule_today_when_window_available() {
    // 今天无视频且窗口可用：不早于 max(08:00, now+2h)。
    let now = at(8, 21, 6, 0, 0); // now+2h=08:00，窗口 [08:00, 19:00)
    let t = pick_schedule_time(&HashMap::new(), now, 0).expect("今天可排");
    assert_eq!(t.date(), day(8, 21));
    assert_eq!(minutes(t), 8 * 60, "rand=0 取下界 08:00");

    let now = at(8, 21, 14, 30, 0); // now+2h=16:30
    let t = pick_schedule_time(&HashMap::new(), now, 0).expect("今天仍可排");
    assert_eq!(t.date(), day(8, 21));
    assert_eq!(minutes(t), 16 * 60 + 30);
}

#[test]
fn schedule_today_rounds_up_seconds_to_next_minute() {
    // now+2h 带秒 -> 向上取整到下一分钟（抖音限制不早于 2 小时后）。
    let now = at(8, 21, 10, 0, 30); // earliest = 12:00:30 -> 12:01
    let t = pick_schedule_time(&HashMap::new(), now, 0).expect("今天可排");
    assert_eq!(minutes(t), 12 * 60 + 1);
}

#[test]
fn schedule_skips_today_when_window_gone() {
    // 17:00 之后 now+2h >= 19:00，今天窗口为空 -> 明天。
    let now = at(8, 21, 17, 0, 0);
    let t = pick_schedule_time(&HashMap::new(), now, 0).expect("明天可排");
    assert_eq!(t.date(), day(8, 22));
    assert!((8 * 60..19 * 60).contains(&minutes(t)));
}

#[test]
fn schedule_crosses_month_boundary() {
    let now = at(8, 31, 10, 0, 0);
    let occ = occupied(&[day(8, 31)]);
    let t = pick_schedule_time(&occ, now, 0).expect("跨月找到");
    assert_eq!(t.date(), day(9, 1));
}

#[test]
fn schedule_skips_today_when_2h_window_crosses_midnight() {
    // 23:00 发起：now+2h 已是明天 01:00，今天任何时刻都违反「2 小时后」-> 明天。
    let now = at(8, 21, 23, 0, 0);
    let t = pick_schedule_time(&HashMap::new(), now, 0).expect("明天可排");
    assert_eq!(t.date(), day(8, 22));
}

#[test]
fn schedule_random_spreads_within_window() {
    // rand_nanos 映射进窗口：取 span-1 时应落在 19:00 前一分钟内。
    let now = at(8, 21, 6, 0, 0); // 窗口 [08:00, 19:00) span=660
    let t = pick_schedule_time(&HashMap::new(), now, 659).expect("今天可排");
    assert_eq!(minutes(t), 18 * 60 + 59);
}

#[test]
fn schedule_pass1_empty_day_beats_pass2() {
    // 层级严格：今天已有 1 个、明天空缺 -> 第一遍优先，选明天而非今天补下午。
    let now = at(8, 21, 6, 0, 0);
    let map = full_days(now.date(), &[10 * 60], &[(1, vec![])]);
    let t = pick_schedule_time(&map, now, 0).expect("明天空缺可排");
    assert_eq!(t.date(), day(8, 22));
    assert_eq!(minutes(t), 8 * 60);
}

#[test]
fn schedule_pass2_fills_afternoon_when_all_days_have_one() {
    // 14 天各有 1 个 10:00（上午）-> 第二遍：今天补下午，rand=0 取段开 12:00。
    let now = at(8, 21, 6, 0, 0);
    let map = full_days(now.date(), &[10 * 60], &[]);
    let t = pick_schedule_time(&map, now, 0).expect("第二遍可排");
    assert_eq!(t.date(), day(8, 21));
    assert_eq!(minutes(t), 12 * 60);
}

#[test]
fn schedule_pass2_afternoon_existing_picks_morning() {
    // 已有 14:00（下午）-> 互补排上午，rand=0 取段开 08:00。
    let now = at(8, 21, 6, 0, 0);
    let map = full_days(now.date(), &[14 * 60], &[]);
    let t = pick_schedule_time(&map, now, 0).expect("第二遍可排");
    assert_eq!(t.date(), day(8, 21));
    assert_eq!(minutes(t), 8 * 60);
}

#[test]
fn schedule_pass2_evening_existing_picks_morning() {
    // 已有 20:00（晚上）-> 顺序第一个空闲段为上午（互补规则的自然延伸）。
    let now = at(8, 21, 6, 0, 0);
    let map = full_days(now.date(), &[20 * 60], &[]);
    let t = pick_schedule_time(&map, now, 0).expect("第二遍可排");
    assert_eq!(t.date(), day(8, 21));
    assert_eq!(minutes(t), 8 * 60);
}

#[test]
fn schedule_pass2_today_afternoon_gone_falls_back_to_evening() {
    // 今天 17:30 发起（earliest=19:30），已有 10:00：下午窗口不足 -> 晚上 [19:30, 22:00)。
    let now = at(8, 21, 17, 30, 0);
    let map = full_days(now.date(), &[10 * 60], &[]);
    let t = pick_schedule_time(&map, now, 0).expect("回退晚上可排");
    assert_eq!(t.date(), day(8, 21));
    assert_eq!(minutes(t), 19 * 60 + 30, "rand=0 取 earliest 19:30");
}

#[test]
fn schedule_pass2_today_morning_gone_falls_back_to_evening() {
    // 今天 11:00 发起（earliest=13:00），已有 14:00：上午已过 -> 晚上 [19:00, 22:00)。
    let now = at(8, 21, 11, 0, 0);
    let map = full_days(now.date(), &[14 * 60], &[]);
    let t = pick_schedule_time(&map, now, 0).expect("回退晚上可排");
    assert_eq!(t.date(), day(8, 21));
    assert_eq!(minutes(t), 19 * 60);
}

#[test]
fn schedule_pass2_skips_today_when_all_slots_gone() {
    // 今天 20:30 发起（earliest=22:30），三段都不足 -> 跳过今天，明天补下午。
    let now = at(8, 21, 20, 30, 0);
    let map = full_days(now.date(), &[10 * 60], &[]);
    let t = pick_schedule_time(&map, now, 0).expect("明天可排");
    assert_eq!(t.date(), day(8, 22));
    assert_eq!(minutes(t), 12 * 60);
}

#[test]
fn schedule_pass2_nearest_day_with_exactly_one() {
    // 今天已 2 个、明天恰 1 个 -> 第二遍选明天（不是更近但已 2 个的今天）。
    let now = at(8, 21, 6, 0, 0);
    let map = full_days(now.date(), &[10 * 60], &[(0, vec![10 * 60, 14 * 60])]);
    let t = pick_schedule_time(&map, now, 0).expect("明天补第二遍");
    assert_eq!(t.date(), day(8, 22));
    assert_eq!(minutes(t), 12 * 60);
}

#[test]
fn schedule_pass3_fills_missing_slot() {
    // 14 天各有 2 个（10:00+14:00，上午+下午）-> 第三遍：今天补晚上 19:00。
    let now = at(8, 21, 6, 0, 0);
    let map = full_days(now.date(), &[10 * 60, 14 * 60], &[]);
    let t = pick_schedule_time(&map, now, 0).expect("第三遍可排");
    assert_eq!(t.date(), day(8, 21));
    assert_eq!(minutes(t), 19 * 60);
}

#[test]
fn schedule_pass3_same_slot_collision_picks_first_free() {
    // 今天 09:00+10:00 撞在上午（手动排的）-> 补顺序第一个空闲段（下午）。
    let now = at(8, 21, 6, 0, 0);
    let map = full_days(now.date(), &[10 * 60, 14 * 60], &[(0, vec![9 * 60, 10 * 60])]);
    let t = pick_schedule_time(&map, now, 0).expect("第三遍可排");
    assert_eq!(t.date(), day(8, 21));
    assert_eq!(minutes(t), 12 * 60);
}

#[test]
fn schedule_none_when_14_days_all_have_three() {
    // 14 天 × 3 个全满 -> None（sidecar 提示人工选择）。
    let now = at(8, 21, 10, 0, 0);
    let map = full_days(now.date(), &[10 * 60, 14 * 60, 20 * 60], &[]);
    assert_eq!(pick_schedule_time(&map, now, 0), None);
}

#[test]
fn publisher_spawn_writes_schedule_time_in_job() {
    let env = setup();
    // 占位「明天」：定时作品 create_time 即定时发布时间（public_time=0）。
    let tomorrow = (chrono::Local::now() + chrono::Duration::days(1)).date_naive();
    let ts = tomorrow
        .and_hms_opt(10, 0, 0)
        .unwrap()
        .and_local_timezone(chrono::Local)
        .unwrap()
        .timestamp();
    DouyinPostsRepo::new(&env.db)
        .upsert(&NewDouyinPost {
            aweme_id: "sched-test-1".to_string(),
            title: None,
            cover_path: None,
            cover_url: None,
            status: "scheduled".to_string(),
            play_count: 0,
            digg_count: 0,
            comment_count: 0,
            share_count: 0,
            completion_rate: None,
            bounce_rate_2s: None,
            violation_status: None,
            violation_text: None,
            violation_brief: None,
            violation_url: None,
            create_time: ts,
            public_time: 0,
        })
        .expect("upsert scheduled post");
    let ctx = AgentContext {
        db: &env.db,
        store: env.store.clone(),
    };
    let task = claim(&env);
    let _ = env.agent.run(&ctx, task).expect("run"); // 首轮 spawn sidecar
    let job: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            env.store
                .root()
                .join("publish")
                .join("jobs")
                .join(format!("{}.json", env.draft_id)),
        )
        .expect("read job"),
    )
    .expect("parse job");
    let s = job["scheduleTime"].as_str().expect("job 应带 scheduleTime");
    let sched =
        NaiveDateTime::parse_from_str(&format!("{s}:00"), "%Y-%m-%d %H:%M:%S").expect("时间格式");
    let now = chrono::Local::now().naive_local();
    assert!(sched > now + chrono::Duration::hours(2), "须不早于 2 小时后: {sched}");
    assert_ne!(sched.date(), tomorrow, "明天已有定时作品，不得再排: {sched}");
    assert!(sched.date() >= now.date(), "不得是过去: {sched}");
    assert!((8 * 60..19 * 60).contains(&minutes(sched)), "须在 08:00~19:00: {sched}");
}
