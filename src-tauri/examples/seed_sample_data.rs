//! 开发期种子数据工具：向 storage/db/app.db 灌入 10 条示例视频资产，
//! 其中 9 条附带模拟 AI 分析结果（覆盖宠物/美食/手工/风景/生活 + 未分类跳过样例），
//! 供第三阶段 GUI 瀑布流、详情面板、筛选与任务列表冒烟测试使用。
//!
//! 用法：
//!   cargo run --example seed_sample_data [sample_video.mp4]
//!   默认示例视频：%TEMP%\e2e_sample.mp4（不存在时报错退出）
//!
//! 幂等性：检测到 dedup_key 前缀为 `seed:` 的资产时直接跳过，避免重复灌入。

use std::path::{Path, PathBuf};

use build_douyin_acount_lib::db::repos::analyses::AnalysesRepo;
use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::domain::analysis::NewAnalysis;
use build_douyin_acount_lib::domain::asset::AssetStatus;
use build_douyin_acount_lib::media::{ImportOptions, MediaStore, VideoManager};
use rusqlite::params;

/// 单条样例数据的声明（元数据与模拟分析结果）。
struct Sample {
    platform: &'static str,
    author: &'static str,
    title: &'static str,
    likes: i64,
    comments: i64,
    plays: i64,
    status: &'static str, // "CANDIDATE" | "new" | "ELIMINATED"
    category: Option<&'static str>,
    tags: &'static [&'static str],
    score: Option<i64>,
    summary: &'static str,
    caption: &'static str,
    foreign: bool,
    english_subtitle: bool,
}

const SAMPLES: &[Sample] = &[
    Sample {
        platform: "instagram",
        author: "@sunny_cat",
        title: "窗台上的橘猫打了个大大的哈欠",
        likes: 182_000,
        comments: 2_340,
        plays: 860_000,
        status: "CANDIDATE",
        category: Some("宠物"),
        tags: &["猫", "萌宠", "治愈", "日常"],
        score: Some(92),
        summary: "一只橘猫在窗台阳光下打哈欠的慢镜头，画面柔和，观感治愈。",
        caption: "谁能拒绝这样一只困困的小猫🥹 阳光、窗台和毛茸茸，是我今天全部的快乐",
        foreign: false,
        english_subtitle: false,
    },
    Sample {
        platform: "tiktok",
        author: "@happy_dog",
        title: "金毛听到开零食的声音瞬间弹射",
        likes: 96_000,
        comments: 1_120,
        plays: 420_000,
        status: "CANDIDATE",
        category: Some("宠物"),
        tags: &["狗", "萌宠", "搞笑"],
        score: Some(85),
        summary: "金毛犬听到零食包装声后从沙发上瞬间弹起，节奏紧凑笑点明确。",
        caption: "这反应速度不去参加运动会可惜了哈哈哈🐕💨",
        foreign: false,
        english_subtitle: false,
    },
    Sample {
        platform: "instagram",
        author: "@chef_xiao",
        title: "三分钟做好一份会流心的煎蛋吐司",
        likes: 245_000,
        comments: 3_890,
        plays: 1_200_000,
        status: "CANDIDATE",
        category: Some("美食"),
        tags: &["做饭", "早餐", "治愈"],
        score: Some(88),
        summary: "第一视角制作流心蛋吐司，步骤清晰，出锅特写很有食欲。",
        caption: "早起十分钟，给自己做一份会流心的幸福🍳✨",
        foreign: false,
        english_subtitle: false,
    },
    Sample {
        platform: "instagram",
        author: "@foodie_map",
        title: "藏在巷子里的老字号烧腊店探店",
        likes: 54_000,
        comments: 860,
        plays: 310_000,
        status: "CANDIDATE",
        category: Some("美食"),
        tags: &["探店", "烧腊", "城市"],
        score: Some(78),
        summary: "街边烧腊店探店记录，老板现场切肉，烟火气十足。",
        caption: "这家开了二十年的烧腊店，排队也要吃🦆🔥",
        foreign: false,
        english_subtitle: false,
    },
    Sample {
        platform: "instagram",
        author: "@handmade_lab",
        title: "旧木板改造：自己动手做一个桌面收纳架",
        likes: 132_000,
        comments: 2_010,
        plays: 640_000,
        status: "CANDIDATE",
        category: Some("手工"),
        tags: &["DIY", "木工", "制作过程"],
        score: Some(83),
        summary: "旧木板改造桌面收纳架全过程，工具步骤清晰，成品实用。",
        caption: "不花一分钱，旧木板秒变高级收纳架🪚",
        foreign: false,
        english_subtitle: false,
    },
    Sample {
        platform: "tiktok",
        author: "@wild_lens",
        title: "晨雾中的森林，光穿过树梢的那一刻",
        likes: 310_000,
        comments: 4_500,
        plays: 1_800_000,
        status: "CANDIDATE",
        category: Some("风景"),
        tags: &["自然", "森林", "治愈"],
        score: Some(90),
        summary: "晨雾森林的延时摄影，光线穿透树梢，画面纯净治愈。",
        caption: "世间万物，都不如这一束穿过森林的光🌲☀️",
        foreign: false,
        english_subtitle: false,
    },
    Sample {
        platform: "instagram",
        author: "@city_walker",
        title: "下雨天的城市街道，霓虹倒映在积水里",
        likes: 78_000,
        comments: 1_050,
        plays: 380_000,
        status: "CANDIDATE",
        category: Some("风景"),
        tags: &["城市", "夜景", "旅行"],
        score: Some(74),
        summary: "雨夜城市街景，霓虹灯倒影与行人剪影，氛围感强。",
        caption: "雨天的城市，比晴天多了一百种颜色🌃",
        foreign: false,
        english_subtitle: false,
    },
    Sample {
        platform: "instagram",
        author: "@daily_vibes",
        title: "周末早晨：咖啡、猫和一本没看完的书",
        likes: 41_000,
        comments: 620,
        plays: 190_000,
        status: "CANDIDATE",
        category: Some("生活"),
        tags: &["日常", "咖啡", "治愈"],
        score: Some(80),
        summary: "周末早晨日常记录，咖啡与书页的细节镜头，节奏舒缓。",
        caption: "周末的正确打开方式：一杯咖啡，一本书☕📖",
        foreign: false,
        english_subtitle: false,
    },
    Sample {
        platform: "instagram",
        author: "@home_stories",
        title: "一家人包饺子的傍晚，厨房里全是笑声",
        likes: 63_000,
        comments: 940,
        plays: 280_000,
        status: "new",
        category: None,
        tags: &[],
        score: None,
        summary: "",
        caption: "",
        foreign: false,
        english_subtitle: false,
    },
    Sample {
        platform: "tiktok",
        author: "@us_talk",
        title: "5 Tips to Grow Your Channel Fast (EN)",
        likes: 460_000,
        comments: 8_200,
        plays: 2_400_000,
        status: "ELIMINATED",
        category: None,
        tags: &["英文口播"],
        score: Some(30),
        summary: "全英文口播内容，大量英文字幕，主要面向海外用户，不适合直接搬运。",
        caption: "",
        foreign: true,
        english_subtitle: true,
    },
];

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root");
    let store = MediaStore::new(root.join("storage"));
    store.ensure_dirs().expect("create storage dirs");
    let db = Database::open(&store.db_dir().join("app.db")).expect("open database");

    // 幂等：已存在 seed 数据则跳过。
    let existing: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM video_assets WHERE dedup_key LIKE 'seed:%'",
            [],
            |r| r.get(0),
        )
        .expect("count seed assets");
    if existing > 0 {
        println!("seed data already present ({existing} assets), skipping.");
        return;
    }

    let video_arg = std::env::args().nth(1);
    let sample = video_arg
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("TEMP").map(|t| PathBuf::from(t).join("e2e_sample.mp4"))
        })
        .expect("sample video path");
    assert!(sample.is_file(), "sample video not found: {}", sample.display());

    let manager = VideoManager::new(&db, store);

    for (i, s) in SAMPLES.iter().enumerate() {
        let asset = manager
            .import_file(
                &sample,
                ImportOptions {
                    source_platform: s.platform.to_string(),
                    title: Some(s.title.to_string()),
                    source_url: Some(format!("https://example.com/seed/{i}")),
                    author: Some(s.author.to_string()),
                    dedup_key: Some(format!("seed:{i:02}")),
                },
            )
            .unwrap_or_else(|e| panic!("import seed {i} failed: {e}"));

        // 补写 import 未覆盖的元数据（种子工具专用直连 SQL，不动生产 API）。
        db.conn()
            .execute(
                "UPDATE video_assets
                 SET tags_json = ?1, likes = ?2, comments = ?3, plays = ?4
                 WHERE id = ?5",
                params![
                    serde_json::to_string(&s.tags).expect("tags json"),
                    s.likes,
                    s.comments,
                    s.plays,
                    asset.id
                ],
            )
            .expect("update seed metadata");

        // 状态流转走领域层校验：NEW -> ANALYZING -> (READY | SKIPPED)。
        if s.status != "new" {
            manager
                .update_status(asset.id, AssetStatus::Analyzing)
                .expect("transition to analyzing");
            let target = match s.status {
                "CANDIDATE" => AssetStatus::Candidate,
                "ELIMINATED" => AssetStatus::Eliminated,
                _ => unreachable!(),
            };
            manager
                .update_status(asset.id, target)
                .expect("transition to target");
        }

        // 写入模拟分析结果（new 状态样例故意不写，模拟未分析素材）。
        if s.status == "CANDIDATE" || s.status == "ELIMINATED" {
            AnalysesRepo::new(&db)
                .upsert(NewAnalysis {
                    asset_id: asset.id,
                    category: s.category.map(str::to_string),
                    tags: s.tags.iter().map(|t| t.to_string()).collect(),
                    quality_score: s.score,
                    has_foreign_language: s.foreign,
                    has_large_amount_english_subtitle: s.english_subtitle,
                    summary: if s.summary.is_empty() {
                        None
                    } else {
                        Some(s.summary.to_string())
                    },
                    recommended_caption: if s.caption.is_empty() {
                        None
                    } else {
                        Some(s.caption.to_string())
                    },
                    asr_text: None,
                    asr_language: None,
                    frame_paths: Vec::new(),
                    model_name: Some("seed-mock".to_string()),
                    prompt_version: Some("dev-seed".to_string()),
                })
                .expect("upsert analysis");
        }

        println!(
            "seeded #{} [{}] {:?} {}",
            asset.id,
            s.platform,
            s.category.or(Some("未分类")),
            s.title
        );
    }
    println!("done: {} sample assets (7 instagram / 3 tiktok).", SAMPLES.len());
}
