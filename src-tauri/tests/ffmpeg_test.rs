use build_douyin_acount_lib::media::MediaStore;
use build_douyin_acount_lib::tools::ffmpeg;

#[test]
fn parse_version_output_extracts_semver() {
    assert_eq!(
        ffmpeg::parse_version_output(
            "ffmpeg version 9.0.1-essentials_build-www.gyan.dev Copyright (c) 2000-2026"
        ),
        Some("9.0.1".to_string())
    );
    assert_eq!(
        ffmpeg::parse_version_output("ffprobe version 8.0-full_build-www.gyan.dev"),
        Some("8.0".to_string())
    );
    // BtbN 的 git N-revision 版本串不可数值化（本项目只用 gyan.dev，无法解析即提示可更新）。
    assert_eq!(ffmpeg::parse_version_output("ffmpeg version N-118207-g5e60aaeeb1"), None);
    assert_eq!(ffmpeg::parse_version_output(""), None);
    assert_eq!(ffmpeg::parse_version_output("ffmpeg version"), None);
}

#[test]
fn update_available_compares_versions_numerically() {
    assert!(ffmpeg::update_available("9.0.1", "9.1.0"));
    assert!(ffmpeg::update_available("8.1.2", "9.0.1"));
    assert!(!ffmpeg::update_available("9.0.1", "9.0.1"), "相同版本无需更新");
    assert!(!ffmpeg::update_available("9.1.0", "9.0.1"), "本地版本更高不更新");
}

/// 造一个与 gyan.dev essentials 同构的 zip（bin/ffmpeg.exe + bin/ffprobe.exe + bin/ffplay.exe）。
fn make_essentials_zip(dir: &std::path::Path) -> std::path::PathBuf {
    use std::io::Write as _;
    let zip_path = dir.join("ffmpeg-release-essentials.zip");
    let file = std::fs::File::create(&zip_path).expect("create zip");
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);
    zw.start_file("ffmpeg-9.0.1-essentials_build/bin/ffmpeg.exe", opts).expect("start ffmpeg");
    zw.write_all(b"new-ffmpeg-binary").expect("write ffmpeg");
    zw.start_file("ffmpeg-9.0.1-essentials_build/bin/ffprobe.exe", opts).expect("start ffprobe");
    zw.write_all(b"new-ffprobe-binary").expect("write ffprobe");
    zw.start_file("ffmpeg-9.0.1-essentials_build/bin/ffplay.exe", opts).expect("start ffplay");
    zw.write_all(b"ignored-ffplay").expect("write ffplay");
    zw.finish().expect("finish zip");
    zip_path
}

#[test]
fn extract_zipped_exes_extracts_ffmpeg_and_ffprobe_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("ensure dirs");
    let zip_path = make_essentials_zip(dir.path());

    let extracted = ffmpeg::extract_zipped_exes(&store, &zip_path).expect("extract");

    assert_eq!(extracted.len(), 2, "只取 ffmpeg 与 ffprobe（不要 ffplay）");
    let ffmpeg_new = ffmpeg::exe_path(&store).with_extension("exe.new");
    let ffprobe_new = ffmpeg::ffprobe_path(&store).with_extension("exe.new");
    assert_eq!(std::fs::read(&ffmpeg_new).expect("read ffmpeg.new"), b"new-ffmpeg-binary");
    assert_eq!(std::fs::read(&ffprobe_new).expect("read ffprobe.new"), b"new-ffprobe-binary");
    assert!(extracted.iter().all(|(exe, _)| exe.parent() == Some(store.tools_dir().as_path())));
}

#[test]
fn extract_zipped_exes_errors_when_ffprobe_missing() {
    use std::io::Write as _;
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("ensure dirs");
    let zip_path = dir.path().join("bad.zip");
    let mut zw = zip::ZipWriter::new(std::fs::File::create(&zip_path).expect("create zip"));
    let opts = zip::write::SimpleFileOptions::default();
    zw.start_file("ffmpeg-9.0.1-essentials_build/bin/ffmpeg.exe", opts).expect("start");
    zw.write_all(b"new-ffmpeg-binary").expect("write");
    zw.finish().expect("finish");

    let err = ffmpeg::extract_zipped_exes(&store, &zip_path).expect_err("缺 ffprobe 必须报错");
    assert!(err.to_string().contains("ffprobe"), "错误应点名缺失文件: {err}");
    assert!(
        !ffmpeg::exe_path(&store).with_extension("exe.new").exists(),
        "失败不留 .new 半成品"
    );
}

#[test]
fn rotate_install_replaces_both_exes_via_rename() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("ensure dirs");
    let ffmpeg = ffmpeg::exe_path(&store);
    let ffprobe = ffmpeg::ffprobe_path(&store);
    std::fs::write(&ffmpeg, b"old-ffmpeg").expect("seed old ffmpeg");
    std::fs::write(&ffprobe, b"old-ffprobe").expect("seed old ffprobe");
    let extracted = vec![
        (ffmpeg.clone(), ffmpeg.with_extension("exe.new")),
        (ffprobe.clone(), ffprobe.with_extension("exe.new")),
    ];
    std::fs::write(&extracted[0].1, b"new-ffmpeg").expect("seed new ffmpeg");
    std::fs::write(&extracted[1].1, b"new-ffprobe").expect("seed new ffprobe");

    ffmpeg::rotate_install(&extracted).expect("rotate");

    assert_eq!(std::fs::read(&ffmpeg).expect("read ffmpeg"), b"new-ffmpeg");
    assert_eq!(std::fs::read(&ffprobe).expect("read ffprobe"), b"new-ffprobe");
    for (exe, new_path) in &extracted {
        assert!(!new_path.exists(), ".new 已被 rename 走");
        assert!(!exe.with_extension("exe.old").exists(), "旧版本已清理");
    }
}

#[test]
fn ensure_from_bundle_copies_only_when_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("ensure dirs");
    let bundle_dir = dir.path().join("bundle");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir");
    std::fs::write(bundle_dir.join("ffmpeg.exe"), b"bundled-ffmpeg").expect("seed ffmpeg");
    std::fs::write(bundle_dir.join("ffprobe.exe"), b"bundled-ffprobe").expect("seed ffprobe");

    // 目标缺失 → 从打包资源目录复制两个 exe。
    ffmpeg::ensure_from_bundle(&store, std::slice::from_ref(&bundle_dir));
    assert_eq!(std::fs::read(ffmpeg::exe_path(&store)).expect("read ffmpeg"), b"bundled-ffmpeg");
    assert_eq!(std::fs::read(ffmpeg::ffprobe_path(&store)).expect("read ffprobe"), b"bundled-ffprobe");

    // 目标已存在（用户一键更新过的新版）→ 绝不覆盖。
    std::fs::write(ffmpeg::exe_path(&store), b"user-updated-ffmpeg").expect("seed updated");
    ffmpeg::ensure_from_bundle(&store, &[bundle_dir]);
    assert_eq!(
        std::fs::read(ffmpeg::exe_path(&store)).expect("read ffmpeg"),
        b"user-updated-ffmpeg",
        "用户更新过的版本不得被内置旧版顶掉"
    );
}
