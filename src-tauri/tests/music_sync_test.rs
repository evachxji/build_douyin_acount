//! 音乐库基础工具（music_sync）回归测试：目录扫描计数与音频扩展名口径。
//! （剪映缓存同步功能 2026-08-27 已由「音乐库采集」取代并整体删除，其测试同步移除；
//! 命名/格式/去重规则的行为守护在 music_collect_test.rs。）

use std::fs;

use build_douyin_acount_lib::music_sync;

#[test]
fn count_music_files_only_counts_audio() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    fs::write(dir.join("a.m4a"), b"x").unwrap();
    fs::write(dir.join("b.MP3"), b"x").unwrap();
    fs::write(dir.join("c.flac"), b"x").unwrap();
    fs::write(dir.join("notes.txt"), b"x").unwrap();
    fs::create_dir_all(dir.join("sub")).unwrap();
    assert_eq!(music_sync::count_music_files(dir), 3);
}

#[test]
fn list_music_returns_audio_files_sorted() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    fs::write(dir.join("b歌.m4a"), b"xx").unwrap();
    fs::write(dir.join("a歌.mp3"), b"x").unwrap();
    fs::write(dir.join("封面.jpg"), b"x").unwrap();
    let files = music_sync::list_music(dir);
    let names: Vec<&str> = files.iter().map(|f| f.file_name.as_str()).collect();
    assert_eq!(names, vec!["a歌.mp3", "b歌.m4a"]);
    assert!(files.iter().all(|f| f.categories.is_empty() && f.cover_path.is_none()));
}
