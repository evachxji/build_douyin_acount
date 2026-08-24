use serde::{Deserialize, Serialize};

/// 抖音创作者中心同步回来的作品快照（发布日历数据源）。
/// status：published / scheduled（public_time 在未来）/ reviewing / private / prohibited / deleted。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DouyinPost {
    pub id: i64,
    pub aweme_id: String,
    pub title: Option<String>,
    pub cover_path: Option<String>,
    pub cover_url: Option<String>,
    pub status: String,
    pub play_count: i64,
    pub digg_count: i64,
    pub comment_count: i64,
    pub share_count: i64,
    /// 完播率（0-100）。列表 API 不返回时为 None，UI 显示 —。
    pub completion_rate: Option<f64>,
    /// 2 秒跳出率（0-100）。同上。
    pub bounce_rate_2s: Option<f64>,
    /// 违规标记：review.status 原值（流量减少=5，健康=2）；非违规为 None。
    pub violation_status: Option<i64>,
    /// 审核结果摘要（如「作品流量减少，优化后可以获得更多流量」）；非空即违规。
    pub violation_text: Option<String>,
    /// 违规原因摘要（如「原创性不足」，来自违规详情接口）。
    pub violation_brief: Option<String>,
    /// 违规申诉/详情页链接。
    pub violation_url: Option<String>,
    /// 创建时间（epoch 秒）。
    pub create_time: i64,
    /// 发布时间（epoch 秒）；定时发布为未来时间。
    pub public_time: i64,
    pub synced_at: String,
    pub updated_at: String,
}

/// 同步 upsert 输入（sidecar posts.jsonl 一行的形状）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewDouyinPost {
    pub aweme_id: String,
    pub title: Option<String>,
    pub cover_path: Option<String>,
    pub cover_url: Option<String>,
    pub status: String,
    pub play_count: i64,
    pub digg_count: i64,
    pub comment_count: i64,
    pub share_count: i64,
    pub completion_rate: Option<f64>,
    pub bounce_rate_2s: Option<f64>,
    pub violation_status: Option<i64>,
    pub violation_text: Option<String>,
    pub violation_brief: Option<String>,
    pub violation_url: Option<String>,
    pub create_time: i64,
    pub public_time: i64,
}
