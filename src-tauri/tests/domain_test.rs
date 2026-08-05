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
