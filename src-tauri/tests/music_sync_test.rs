//! 剪映音乐同步（music_sync）回归测试：用 tempfile 造假剪映缓存目录
//! （music/ 音频 + downLoadcfg 索引 + ressdk_db 里的 rp.db），不依赖真实剪映安装。

use std::fs;
use std::path::{Path, PathBuf};

use build_douyin_acount_lib::music_sync;
use md5::{Digest, Md5};
use rusqlite::Connection;

/// M4A(AAC) 文件头：第 4-8 字节是 ftyp（剪映缓存的 .mp3 实际多是这个）。
const M4A_HEAD: &[u8] = b"\x00\x00\x00\x20ftypM4A \x00\x00\x00\x00";
/// MP3 文件头：ID3v2 标签。
const MP3_HEAD: &[u8] = b"ID3\x04\x00\x00\x00\x00\x00\x23";

fn md5_hex(text: &str) -> String {
    format!("{:x}", Md5::digest(text.as_bytes()))
}

fn write_audio(music_dir: &Path, name: &str, head: &[u8], fill: u8) {
    let mut content = head.to_vec();
    content.extend_from_slice(&[fill; 64]);
    fs::write(music_dir.join(name), content).expect("write audio");
}

fn write_download_cfg(music_dir: &Path, entries: &[(&str, &str)]) {
    let list: Vec<serde_json::Value> = entries
        .iter()
        .map(|(hex, path)| serde_json::json!({ "hex": hex, "path": path }))
        .collect();
    fs::write(
        music_dir.join("downLoadcfg"),
        serde_json::json!({ "list": list }).to_string(),
    )
    .expect("write downLoadcfg");
}

/// 在 cache_root/ressdk_db/res/rp.db 落库：http_cache 响应体 + music 表行。
fn write_rp_db(cache_root: &Path, http_bodies: &[String], music_rows: &[(&str, &str, &str)]) {
    let dir = cache_root.join("ressdk_db").join("res");
    fs::create_dir_all(&dir).expect("ressdk_db dir");
    let con = Connection::open(dir.join("rp.db")).expect("open rp.db");
    con.execute("CREATE TABLE http_cache (response_body TEXT)", []).unwrap();
    for b in http_bodies {
        con.execute("INSERT INTO http_cache (response_body) VALUES (?1)", [b]).unwrap();
    }
    con.execute("CREATE TABLE music (id TEXT, title TEXT, author TEXT)", []).unwrap();
    for (id, title, author) in music_rows {
        con.execute("INSERT INTO music (id, title, author) VALUES (?1, ?2, ?3)", [id, title, author])
            .unwrap();
    }
}

/// 返回 (临时目录守卫, cache_root, 目标目录)。
fn fake_cache() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cache = tmp.path().join("Cache");
    fs::create_dir_all(cache.join("music")).expect("music cache dir");
    let dest = tmp.path().join("out");
    (tmp, cache, dest)
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read dest dir")
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

#[test]
fn sync_renames_by_song_index_and_detects_real_format() {
    let (_tmp, cache, dest) = fake_cache();
    let music = cache.join("music");
    write_audio(&music, "a1b2.mp3", M4A_HEAD, 0x11);
    write_audio(&music, "c3d4.mp3", MP3_HEAD, 0x22);
    write_rp_db(
        &cache,
        &[serde_json::json!({
            "data": { "songs": [
                { "id": 123, "title": "歌名A", "author": "作者甲" },
                { "id": "456", "title": "歌名B", "author": null }
            ] }
        })
        .to_string()],
        &[],
    );
    write_download_cfg(&music, &[(&md5_hex("123"), "a1b2.mp3"), (&md5_hex("456"), "c3d4.mp3")]);

    let report = music_sync::sync(&cache, &dest).expect("sync");

    assert_eq!(report.copied, 2);
    assert_eq!(report.skipped, 0);
    assert_eq!(report.unmatched, 0);
    // 数字与字符串两种 id 形态都命中；扩展名按文件头探测（.mp3 → .m4a 修正）。
    assert_eq!(file_names(&dest), vec!["歌名A - 作者甲.m4a", "歌名B.mp3"]);
}

#[test]
fn sync_preserves_19_digit_song_id_precision() {
    // 抖音歌曲 ID 是 19 位数字（超 i64::MAX）：JSON 解析成 f64 会丢精度，
    // md5(id) 对不上 downLoadcfg 的 hex，文件会退化成哈希名——本用例守护 RawValue 原文路径。
    let (_tmp, cache, dest) = fake_cache();
    let music = cache.join("music");
    write_audio(&music, "e5f6.mp3", M4A_HEAD, 0x33);
    let big_id = "7188308046678695949";
    write_rp_db(
        &cache,
        &[format!(r#"{{"data":{{"songs":[{{"id":{big_id},"title":"大数歌","author":"乙"}}]}}}}"#)],
        &[],
    );
    write_download_cfg(&music, &[(&md5_hex(big_id), "e5f6.mp3")]);

    let report = music_sync::sync(&cache, &dest).expect("sync");

    assert_eq!(report.unmatched, 0, "19 位 ID 必须精确命中歌名索引");
    assert_eq!(file_names(&dest), vec!["大数歌 - 乙.m4a"]);
}

#[test]
fn sync_reads_music_table_fallback() {
    // 来源2：部分剪映版本把歌曲直接落 music 表（http_cache 为空）。
    let (_tmp, cache, dest) = fake_cache();
    let music = cache.join("music");
    write_audio(&music, "7788.mp3", MP3_HEAD, 0x44);
    write_rp_db(&cache, &[], &[("789", "表内歌", "丙")]);
    write_download_cfg(&music, &[(&md5_hex("789"), "7788.mp3")]);

    let report = music_sync::sync(&cache, &dest).expect("sync");

    assert_eq!(report.unmatched, 0);
    assert_eq!(file_names(&dest), vec!["表内歌 - 丙.mp3"]);
}

#[test]
fn sync_skips_same_content_and_appends_suffix_for_different() {
    let (_tmp, cache, dest) = fake_cache();
    let music = cache.join("music");
    write_audio(&music, "aa00.mp3", M4A_HEAD, 0x55);
    write_rp_db(&cache, &[], &[("321", "重复歌", "丁")]);
    write_download_cfg(&music, &[(&md5_hex("321"), "aa00.mp3")]);

    let first = music_sync::sync(&cache, &dest).expect("first sync");
    assert_eq!((first.copied, first.skipped), (1, 0));

    // 幂等：同内容重跑全部跳过。
    let second = music_sync::sync(&cache, &dest).expect("second sync");
    assert_eq!((second.copied, second.skipped), (0, 1));
    assert_eq!(file_names(&dest), vec!["重复歌 - 丁.m4a"]);

    // 同名不同内容（换了音频但歌名相同）→ 追加序号而非覆盖。
    write_audio(&music, "aa00.mp3", M4A_HEAD, 0x66);
    let third = music_sync::sync(&cache, &dest).expect("third sync");
    assert_eq!((third.copied, third.skipped), (1, 0));
    assert_eq!(file_names(&dest), vec!["重复歌 - 丁 (2).m4a", "重复歌 - 丁.m4a"]);
}

#[test]
fn sync_skips_unmatched_files() {
    // 未匹配歌名的文件不同步（2026-08-26 用户明确：哈希名的不要同步过来）。
    let (_tmp, cache, dest) = fake_cache();
    let music = cache.join("music");
    write_audio(&music, "9ab8c7.mp3", M4A_HEAD, 0x77);
    write_rp_db(&cache, &[], &[]);
    write_download_cfg(&music, &[("deadbeef", "9ab8c7.mp3")]);

    let report = music_sync::sync(&cache, &dest).expect("sync");

    assert_eq!(report.copied, 0);
    assert_eq!(report.unmatched, 1);
    assert!(file_names(&dest).is_empty(), "未匹配歌名的文件不得复制到目标目录");
}

#[test]
fn sync_sanitizes_illegal_chars_in_title() {
    let (_tmp, cache, dest) = fake_cache();
    let music = cache.join("music");
    write_audio(&music, "bb11.mp3", MP3_HEAD, 0x88);
    write_audio(&music, "dd33.mp3", MP3_HEAD, 0x99);
    write_rp_db(
        &cache,
        &[serde_json::json!({
            "data": { "songs": [
                { "id": 654, "title": "a/b:c*d?e\"f<g>h|i", "author": "戊" },
                { "id": 655, "title": ".. 孤勇者 . ", "author": null }
            ] }
        })
        .to_string()],
        &[],
    );
    write_download_cfg(&music, &[(&md5_hex("654"), "bb11.mp3"), (&md5_hex("655"), "dd33.mp3")]);

    let report = music_sync::sync(&cache, &dest).expect("sync");

    assert_eq!(report.copied, 2);
    // 非法字符 → _；无作者时只留歌名，两端空格与点被 strip。
    assert_eq!(file_names(&dest), vec!["a_b_c_d_e_f_g_h_i - 戊.mp3", "孤勇者.mp3"]);
}

#[test]
fn sync_falls_back_to_dir_scan_without_downloadcfg() {
    let (_tmp, cache, dest) = fake_cache();
    let music = cache.join("music");
    write_audio(&music, "cc22.mp3", MP3_HEAD, 0x99);
    fs::write(music.join("cc22.beat"), b"beat-markers").expect("write beat file");

    let report = music_sync::sync(&cache, &dest).expect("sync");

    // 无索引 → 扫目录兜底，但 hex 为空必然全部未匹配歌名 → 全部跳过不复制。
    assert_eq!((report.copied, report.unmatched), (0, 1));
    assert!(file_names(&dest).is_empty());
}

#[test]
fn sync_fails_when_cache_dir_missing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let result = music_sync::sync(tmp.path(), &tmp.path().join("out"));
    let err = result.expect_err("music 缓存目录不存在必须报错");
    assert!(err.to_string().contains("未找到剪映音乐缓存目录"), "错误信息要指明缺失目录: {err}");
}

#[test]
fn sync_extracts_exact_cover_url_from_jianying_cache() {
    // 剪映缓存的 cover_url 是字符串化字典（"{'hd': 'https://…'}"）；music 表则是独立列。
    let (_tmp, cache, dest) = fake_cache();
    let music = cache.join("music");
    write_audio(&music, "c001.mp3", M4A_HEAD, 0x12);
    write_audio(&music, "c002.mp3", MP3_HEAD, 0x34);
    write_rp_db(
        &cache,
        &[serde_json::json!({
            "data": { "songs": [
                { "id": 111, "title": "缓存歌", "author": "甲",
                  "cover_url": "{'hd': 'https://p9-lv-music-img.faceu.mobi/aweme/1080x1080/abc', 'medium': 'https://x/m.jpg'}" }
            ] }
        })
        .to_string()],
        &[("222", "表歌", "乙")],
    );
    write_download_cfg(&music, &[(&md5_hex("111"), "c001.mp3"), (&md5_hex("222"), "c002.mp3")]);

    let report = music_sync::sync(&cache, &dest).expect("sync");

    assert_eq!(report.copied, 2);
    assert_eq!(
        report.cover_urls,
        vec![(
            "缓存歌 - 甲.m4a".to_string(),
            "https://p9-lv-music-img.faceu.mobi/aweme/1080x1080/abc".to_string()
        )],
        "http_cache 的字符串化字典要抽出 hd 直链；无封面的 music 表行不出现在映射里"
    );
}

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
