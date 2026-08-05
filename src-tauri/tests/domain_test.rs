use build_douyin_acount_lib::domain::asset::AssetStatus;
use build_douyin_acount_lib::domain::task::TaskStatus;

#[test]
fn asset_status_allows_documented_transitions() {
    // NEW -> ANALYZING / SKIPPED / DOWNLOAD_FAILED
    assert!(AssetStatus::New.can_transition_to(&AssetStatus::Analyzing));
    assert!(AssetStatus::New.can_transition_to(&AssetStatus::Skipped));
    assert!(AssetStatus::New.can_transition_to(&AssetStatus::DownloadFailed));
    // ANALYZING -> READY / LOW_PRIORITY / NEW(重试)
    assert!(AssetStatus::Analyzing.can_transition_to(&AssetStatus::Ready));
    assert!(AssetStatus::Analyzing.can_transition_to(&AssetStatus::LowPriority));
    assert!(AssetStatus::Analyzing.can_transition_to(&AssetStatus::New));
    // READY -> SELECTED / ANALYZING(重分析) / SKIPPED
    assert!(AssetStatus::Ready.can_transition_to(&AssetStatus::Selected));
    assert!(AssetStatus::Ready.can_transition_to(&AssetStatus::Analyzing));
    // SELECTED -> PUBLISHED / READY(取消选择)
    assert!(AssetStatus::Selected.can_transition_to(&AssetStatus::Published));
    assert!(AssetStatus::Selected.can_transition_to(&AssetStatus::Ready));
    // PUBLISHED -> READY(纠错)
    assert!(AssetStatus::Published.can_transition_to(&AssetStatus::Ready));
    // SKIPPED / LOW_PRIORITY 可恢复
    assert!(AssetStatus::Skipped.can_transition_to(&AssetStatus::Ready));
    assert!(AssetStatus::LowPriority.can_transition_to(&AssetStatus::Ready));
    // DOWNLOAD_FAILED -> NEW 重试
    assert!(AssetStatus::DownloadFailed.can_transition_to(&AssetStatus::New));
}

#[test]
fn asset_status_rejects_illegal_transitions() {
    assert!(!AssetStatus::New.can_transition_to(&AssetStatus::Published));
    assert!(!AssetStatus::New.can_transition_to(&AssetStatus::Ready));
    assert!(!AssetStatus::Published.can_transition_to(&AssetStatus::Selected));
    assert!(!AssetStatus::DownloadFailed.can_transition_to(&AssetStatus::Published));
}

#[test]
fn task_status_allows_documented_transitions() {
    assert!(TaskStatus::Pending.can_transition_to(&TaskStatus::Running));
    assert!(TaskStatus::Running.can_transition_to(&TaskStatus::Succeeded));
    assert!(TaskStatus::Running.can_transition_to(&TaskStatus::Failed));
    assert!(TaskStatus::Running.can_transition_to(&TaskStatus::WaitingForHuman));
    assert!(TaskStatus::WaitingForHuman.can_transition_to(&TaskStatus::Running));
    assert!(TaskStatus::WaitingForHuman.can_transition_to(&TaskStatus::Cancelled));
    assert!(TaskStatus::Failed.can_transition_to(&TaskStatus::Pending)); // 重试
}

#[test]
fn task_status_rejects_illegal_transitions() {
    assert!(!TaskStatus::Pending.can_transition_to(&TaskStatus::Succeeded));
    assert!(!TaskStatus::Succeeded.can_transition_to(&TaskStatus::Running));
    assert!(!TaskStatus::Cancelled.can_transition_to(&TaskStatus::Running));
}

#[test]
fn asset_status_serde_roundtrip_matches_database_strings() {
    // 序列化结果必须与 DB/Display 字符串一致（LOW_PRIORITY / DOWNLOAD_FAILED 带下划线），
    // 否则前端状态徽章与筛选会拿到 LOWPRIORITY 之类的裸值。
    let cases: &[(AssetStatus, &str)] = &[
        (AssetStatus::New, "NEW"),
        (AssetStatus::Analyzing, "ANALYZING"),
        (AssetStatus::Ready, "READY"),
        (AssetStatus::Selected, "SELECTED"),
        (AssetStatus::Published, "PUBLISHED"),
        (AssetStatus::Skipped, "SKIPPED"),
        (AssetStatus::LowPriority, "LOW_PRIORITY"),
        (AssetStatus::DownloadFailed, "DOWNLOAD_FAILED"),
    ];
    for (status, expected) in cases {
        let json = serde_json::to_string(status).expect("serialize");
        assert_eq!(json, format!("\"{expected}\""), "status {expected}");
        let back: AssetStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, *status);
        assert_eq!(status.to_string(), *expected, "Display must match DB string");
    }
}

#[test]
fn draft_status_lifecycle_and_serde() {
    use build_douyin_acount_lib::domain::draft::DraftStatus;

    // DRAFT -> FILLED -> PUBLISHED；任意阶段可取消
    assert!(DraftStatus::Draft.can_transition_to(&DraftStatus::Filled));
    assert!(DraftStatus::Draft.can_transition_to(&DraftStatus::Cancelled));
    assert!(DraftStatus::Filled.can_transition_to(&DraftStatus::Published));
    assert!(DraftStatus::Filled.can_transition_to(&DraftStatus::Cancelled));
    assert!(!DraftStatus::Draft.can_transition_to(&DraftStatus::Draft));
    assert!(!DraftStatus::Published.can_transition_to(&DraftStatus::Filled));

    // serde 输出与 DB 字符串一致
    for (status, expected) in [
        (DraftStatus::Draft, "DRAFT"),
        (DraftStatus::Filled, "FILLED"),
        (DraftStatus::Published, "PUBLISHED"),
        (DraftStatus::Cancelled, "CANCELLED"),
    ] {
        let json = serde_json::to_string(&status).expect("serialize");
        assert_eq!(json, format!("\"{expected}\""));
        let back: DraftStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, status);
        assert_eq!(status.to_string(), expected);
    }
}
