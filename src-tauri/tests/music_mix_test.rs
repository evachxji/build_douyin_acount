use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use build_douyin_acount_lib::db::repos::assets::AssetsRepo;
use build_douyin_acount_lib::db::Database;
use build_douyin_acount_lib::domain::asset::{AssetStatus, NewAsset};
use build_douyin_acount_lib::domain::error::DomainError;
use build_douyin_acount_lib::media::{MediaStore, VideoManager};
use build_douyin_acount_lib::music_mix::{self, MixOptions};

/// 保留原声 + 默认音量（30/70）与关原声两套常用选项。
const MIX: MixOptions = MixOptions { mute: false, video_volume: 30, music_volume: 70 };
const MUTE: MixOptions = MixOptions { mute: true, video_volume: 30, music_volume: 70 };

fn has_tool(name: &str) -> bool {
    Command::new(name)
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn setup() -> (tempfile::TempDir, Database, MediaStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("dirs");
    let db = Database::open(&dir.path().join("storage/db/app.db")).expect("db");
    (dir, db, store)
}

fn fixture_video() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join("sample_1s.mp4")
}

/// 把视频复制进媒体库并入库，状态流转到 CANDIDATE，返回资产 id 与库内视频路径。
fn seed_candidate(db: &Database, store: &MediaStore, video_src: &Path) -> (i64, PathBuf) {
    let video = store.videos_dir().join("seed-video.mp4");
    std::fs::copy(video_src, &video).expect("copy video");
    let id = AssetsRepo::new(db)
        .insert(NewAsset {
            dedup_key: None,
            file_path: video.to_string_lossy().into_owned(),
            cover_path: None,
            source_platform: "local".to_string(),
            source_url: None,
            author: None,
            title: Some("测试视频".to_string()),
            description: None,
            published_at: None,
            tags: Vec::new(),
            likes: 0,
            comments: 0,
            plays: 0,
            duration_ms: 1000,
        })
        .expect("insert asset");
    let repo = AssetsRepo::new(db);
    repo.update_status(id, AssetStatus::Analyzing).expect("to analyzing");
    repo.update_status(id, AssetStatus::Candidate).expect("to candidate");
    (id, video)
}

fn seed_music(store: &MediaStore, name: &str) -> PathBuf {
    let music = store.music_dir().join(name);
    std::fs::write(&music, b"fake music bytes").expect("write music");
    music
}

/// 假 ffmpeg：记录调用参数并直接写输出文件（args 最后一个元素是输出路径）。
fn fake_exec(calls: &Mutex<Vec<Vec<String>>>) -> impl Fn(&str, &[String]) -> Result<Vec<u8>, DomainError> + '_ {
    move |_name: &str, args: &[String]| -> Result<Vec<u8>, DomainError> {
        calls.lock().expect("calls lock").push(args.to_vec());
        std::fs::write(args.last().expect("out path"), b"fake merged video")?;
        Ok(Vec::new())
    }
}

// ---------- build_merge_args 纯函数 ----------

#[test]
fn merge_args_mute_maps_video_and_music_only() {
    let args = music_mix::build_merge_args(
        Path::new("v.mp4"),
        Path::new("m.m4a"),
        Path::new("out.mp4"),
        12.5,
        true,
        MUTE,
    );
    let joined = args.join(" ");
    assert!(!joined.contains("filter_complex"), "关原声不需要混音滤镜: {joined}");
    assert!(joined.contains("-map 0:v:0 -map 1:a:0"), "只映射视频流+音乐流: {joined}");
    assert!(joined.contains("-t 12.500"), "音乐裁到视频时长: {joined}");
    assert!(joined.contains("-c:v copy"), "视频流 copy 不重编码: {joined}");
}

#[test]
fn merge_args_mix_applies_custom_volumes() {
    let args = music_mix::build_merge_args(
        Path::new("v.mp4"),
        Path::new("m.m4a"),
        Path::new("out.mp4"),
        12.0,
        true,
        MIX,
    );
    let joined = args.join(" ");
    assert!(joined.contains("[0:a]volume=0.30"), "原声按滑条音量（默认 30%）: {joined}");
    assert!(joined.contains("[1:a]volume=0.70"), "音乐按滑条音量（默认 70%）: {joined}");
    assert!(joined.contains("amix=inputs=2"), "原声+音乐混音: {joined}");
    assert!(joined.contains("duration=first"), "混音长度跟随原声: {joined}");
    assert!(joined.contains("-map [a]"), "输出混音轨: {joined}");

    // 用户自定义音量（如原声 50%、音乐 80%）如实进滤镜。
    let custom = music_mix::build_merge_args(
        Path::new("v.mp4"),
        Path::new("m.m4a"),
        Path::new("out.mp4"),
        12.0,
        true,
        MixOptions { mute: false, video_volume: 50, music_volume: 80 },
    )
    .join(" ");
    assert!(custom.contains("volume=0.50"), "自定义原声音量: {custom}");
    assert!(custom.contains("volume=0.80"), "自定义音乐音量: {custom}");
}

#[test]
fn merge_args_no_audio_source_falls_back_to_music_only() {
    // 源视频无音轨时 amix 会报错，必须退化为直接铺音乐（与关原声同构）。
    let mute_args = music_mix::build_merge_args(Path::new("v.mp4"), Path::new("m.m4a"), Path::new("o.mp4"), 1.0, true, MUTE);
    let no_audio_args = music_mix::build_merge_args(Path::new("v.mp4"), Path::new("m.m4a"), Path::new("o.mp4"), 1.0, false, MIX);
    assert_eq!(mute_args, no_audio_args);
}

// ---------- apply_music（假 ffmpeg） ----------

#[test]
fn apply_music_rejects_non_candidate_status() {
    let (_dir, db, store) = setup();
    let (id, _video) = seed_candidate(&db, &store, &fixture_video());
    AssetsRepo::new(&db)
        .update_status(id, AssetStatus::Analyzing)
        .expect("back to analyzing");
    let music = seed_music(&store, "a.m4a");
    let calls: Mutex<Vec<Vec<String>>> = Mutex::new(Vec::new());

    let err = music_mix::apply_music(&db, &store, id, &music, MIX, &fake_exec(&calls))
        .expect_err("非候选/待发布必须拒绝");

    assert!(err.to_string().contains("候选"), "错误应说明状态限制: {err}");
}

#[test]
fn apply_music_rejects_music_outside_library() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("dirs");
    let db = Database::open(&dir.path().join("storage/db/app.db")).expect("db");
    let (id, _video) = seed_candidate(&db, &store, &fixture_video());
    // 音乐在媒体库目录之外。
    let outside = dir.path().join("outside.m4a");
    std::fs::write(&outside, b"fake").expect("write outside music");
    let calls: Mutex<Vec<Vec<String>>> = Mutex::new(Vec::new());

    let err = music_mix::apply_music(&db, &store, id, &outside, MIX, &fake_exec(&calls))
        .expect_err("目录外音乐必须拒绝");

    assert!(err.to_string().contains("音乐库"), "错误应说明目录限制: {err}");
}

#[test]
fn apply_music_first_time_records_origin_and_remerge_uses_origin() {
    if !has_tool("ffprobe") {
        eprintln!("skipped: ffprobe not available");
        return;
    }
    let (_dir, db, store) = setup();
    let (id, origin_video) = seed_candidate(&db, &store, &fixture_video());
    let music_a = seed_music(&store, "Song A - x.m4a");
    let music_b = seed_music(&store, "Song B - y.m4a");
    let calls: Mutex<Vec<Vec<String>>> = Mutex::new(Vec::new());

    // 首次合成（保留原声）。
    let r1 = music_mix::apply_music(&db, &store, id, &music_a, MIX, &fake_exec(&calls))
        .expect("first apply");
    let merged_path = store.videos_dir().join("seed-video.music.mp4");
    assert_eq!(r1.file_path, merged_path.to_string_lossy());
    assert_eq!(r1.music_info.origin_file_path, origin_video.to_string_lossy());
    assert_eq!(r1.music_info.music_name, "Song A - x.m4a");
    assert!(!r1.music_info.mute_original);
    assert_eq!((r1.music_info.video_volume, r1.music_info.music_volume), (30, 70), "音量随合成入库");
    // db 里的 file_path/music_info 已更新。
    let asset = AssetsRepo::new(&db).get(id).expect("get").expect("exists");
    assert_eq!(asset.file_path, merged_path.to_string_lossy());
    assert_eq!(asset.music_info.expect("music_info").music_name, "Song A - x.m4a");
    // 首次合成的输入是原始视频，-t 是真实探测时长（sample_1s ≈ 1s）。
    // （一次性取走快照再断言：持锁引用会让下一次 lock 同线程死锁。）
    let recorded = calls.lock().expect("calls").clone();
    let first_args = &recorded[0];
    let t_pos = first_args.iter().position(|a| a == "-t").expect("-t present");
    let t: f64 = first_args[t_pos + 1].parse().expect("t number");
    assert!((0.5..1.5).contains(&t), "时长应来自 ffprobe 实测: {t}");
    assert!(first_args[2] == origin_video.to_string_lossy(), "输入应是原始视频");

    // 再次合成（换音乐 + 关原声）：必须从原始视频重来，不叠音。
    let r2 = music_mix::apply_music(&db, &store, id, &music_b, MUTE, &fake_exec(&calls))
        .expect("second apply");
    assert_eq!(r2.music_info.music_name, "Song B - y.m4a");
    assert!(r2.music_info.mute_original);
    assert_eq!(r2.music_info.origin_file_path, origin_video.to_string_lossy(), "origin 不变");
    let recorded = calls.lock().expect("calls").clone();
    let second_args = &recorded[1];
    assert_eq!(second_args[2], origin_video.to_string_lossy(), "再次合成输入仍是原始视频（不叠音）");
    assert!(!second_args.join(" ").contains("filter_complex"), "关原声直接铺音乐");
}

// ---------- 删除清理 ----------

#[test]
fn music_info_old_json_defaults_volumes_to_30_70() {
    // 音量功能上线前的旧 music_info_json 没有音量字段：反序列化必须回填默认值 30/70。
    use build_douyin_acount_lib::domain::asset::MusicInfo;
    let info: MusicInfo = serde_json::from_str(
        r#"{"origin_file_path":"a.mp4","music_name":"x.m4a","mute_original":true}"#,
    )
    .expect("old json parses");
    assert_eq!((info.video_volume, info.music_volume), (30, 70));
}

#[test]
fn delete_asset_removes_origin_and_merged_files() {
    let (_dir, db, store) = setup();
    let (id, origin_video) = seed_candidate(&db, &store, &fixture_video());
    // 直接写 DB 模拟已合成状态（不依赖 ffprobe）。
    let merged = store.videos_dir().join("seed-video.music.mp4");
    std::fs::write(&merged, b"merged").expect("write merged");
    let info = serde_json::json!({
        "origin_file_path": origin_video.to_string_lossy(),
        "music_name": "a.m4a",
        "mute_original": false,
    });
    AssetsRepo::new(&db)
        .apply_music(id, &merged.to_string_lossy(), &info.to_string())
        .expect("apply_music repo");
    assert!(origin_video.exists() && merged.exists());

    VideoManager::new(&db, store.clone())
        .delete_asset(id)
        .expect("delete");

    assert!(!merged.exists(), "合成产物已删除");
    assert!(!origin_video.exists(), "原始视频也一并删除");
}

// ---------- 真 ffmpeg 冒烟（环境无 ffmpeg 时跳过） ----------

#[test]
fn real_ffmpeg_merge_trims_music_to_video_duration() {
    if !has_tool("ffmpeg") || !has_tool("ffprobe") {
        eprintln!("skipped: ffmpeg/ffprobe not available");
        return;
    }
    let (_dir, db, store) = setup();
    // 现场生成 1s 视频（带音轨）与 2s 音乐。
    let video = store.videos_dir().join("real.mp4");
    let ok = Command::new("ffmpeg")
        .args([
            "-y", "-f", "lavfi", "-i", "testsrc=duration=1:size=320x240:rate=10",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=1",
            "-pix_fmt", "yuv420p",
        ])
        .arg(&video)
        .output()
        .expect("gen video");
    assert!(ok.status.success(), "生成测试视频失败");
    let music = store.music_dir().join("real-music.m4a");
    let ok = Command::new("ffmpeg")
        .args(["-y", "-f", "lavfi", "-i", "sine=frequency=880:duration=2"])
        .arg(&music)
        .output()
        .expect("gen music");
    assert!(ok.status.success(), "生成测试音乐失败");
    let (id, _) = seed_candidate(&db, &store, &video);

    // 混音路径（保留原声 + 音乐 25%）走真 ffmpeg。
    let r = music_mix::apply_music(&db, &store, id, &music, MIX, &music_mix::ffmpeg_exec)
        .expect("real merge");

    // 输出时长 ≈ 1s（2s 音乐被裁到视频长度），且带音轨。
    let out = PathBuf::from(&r.file_path);
    assert!(out.is_file());
    let probe = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .arg(&out)
        .output()
        .expect("probe out");
    let duration: f64 = String::from_utf8_lossy(&probe.stdout).trim().parse().expect("duration");
    assert!((0.7..1.4).contains(&duration), "输出时长应≈视频时长 1s，实际 {duration}");
    let probe = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries", "stream=codec_name", "-of", "csv=p=0"])
        .arg(&out)
        .output()
        .expect("probe audio");
    assert!(String::from_utf8_lossy(&probe.stdout).contains("aac"), "输出应有 AAC 音轨");
}
