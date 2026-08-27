//! 音乐库采集真实联调：对剪映线上音乐库 API 跑一轮小规模采集（默认每分类 1 首，
//! 验证 Rust 链路对真实服务的协议细节；想全量采，在应用里用「设置 → 音乐库采集」）。
//!
//! 用法：
//!   cargo run --example e2e_music_collect -- [每分类数量]
//!
//! 产物直接落在仓库 storage/music/ 并写 music_categories 表（与应用内采集同一落点，幂等）。

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::media::MediaStore;
use build_douyin_acount_lib::music_collect;

fn main() {
    let per_category: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root");
    let store = MediaStore::new(root.join("storage"));
    store.ensure_dirs().expect("create storage dirs");
    let db = Arc::new(Database::open(&store.db_dir().join("app.db")).expect("open database"));
    let pp = store.music_dir().join(".collect_progress.json");
    let cancel = AtomicBool::new(false);

    println!("开始采集：每分类 {per_category} 首 → {}", store.music_dir().display());
    music_collect::run_collect(&db, &store, per_category, music_collect::API_BASE, &pp, &cancel)
        .expect("run_collect");

    let p = music_collect::collect_status(&store).expect("progress");
    println!(
        "完成：stage={} 新下载={} 跳过={} 失败={} 计划={} | {}",
        p.stage, p.downloaded, p.skipped, p.failed, p.planned_total, p.message
    );
}
