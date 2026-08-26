use build_douyin_acount_lib::media::MediaStore;
use build_douyin_acount_lib::tools::ytdlp;

#[test]
fn update_available_compares_versions_numerically() {
    assert!(ytdlp::update_available("2025.08.11", "2025.08.22"));
    assert!(!ytdlp::update_available("2025.08.22", "2025.08.22"), "相同版本无需更新");
    assert!(!ytdlp::update_available("2025.09.01", "2025.08.22"), "本地版本更高不更新");
    // 无前导零写法（"2025.8.1"）按字符串比较会被误判大于 "2025.08.22"，必须数值化。
    assert!(ytdlp::update_available("2025.8.1", "2025.08.22"));
    assert!(!ytdlp::update_available("2025.8.22", "2025.08.11"));
    assert!(!ytdlp::update_available("2025.8.22", "2025.08.22"), "前导零差异视为同版");
}

#[test]
fn install_replaces_exe_via_rename() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MediaStore::new(dir.path().to_path_buf());
    store.ensure_dirs().expect("ensure dirs");
    let exe = ytdlp::exe_path(&store);
    std::fs::write(&exe, b"old-binary").expect("seed old exe");
    let new = exe.with_extension("exe.new");
    std::fs::write(&new, b"new-binary").expect("seed new exe");

    ytdlp::install(&store, &new).expect("install");

    assert_eq!(std::fs::read(&exe).expect("read exe"), b"new-binary");
    assert!(!new.exists(), "new 已被 rename 走");
    assert!(!exe.with_extension("exe.old").exists(), "旧版本已清理");
}

#[test]
fn install_without_existing_exe_places_new_file() {
    // 未安装时（设置页「下载安装」路径）：无 exe 可轮换，直接落位。
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MediaStore::new(dir.path().to_path_buf());
    store.ensure_dirs().expect("ensure dirs");
    let exe = ytdlp::exe_path(&store);
    let new = exe.with_extension("exe.new");
    std::fs::write(&new, b"new-binary").expect("seed new exe");

    ytdlp::install(&store, &new).expect("install");

    assert_eq!(std::fs::read(&exe).expect("read exe"), b"new-binary");
}

#[test]
fn ensure_from_bundle_copies_only_when_missing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = MediaStore::new(dir.path().join("storage"));
    store.ensure_dirs().expect("ensure dirs");
    let bundle_dir = dir.path().join("bundle");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir");
    let bundled = bundle_dir.join("yt-dlp.exe");
    std::fs::write(&bundled, b"bundled-binary").expect("seed bundled exe");

    // 目标缺失 → 从打包资源复制。
    ytdlp::ensure_from_bundle(&store, std::slice::from_ref(&bundled));
    assert_eq!(
        std::fs::read(ytdlp::exe_path(&store)).expect("read exe"),
        b"bundled-binary"
    );

    // 目标已存在（用户一键更新过的新版）→ 绝不覆盖。
    std::fs::write(ytdlp::exe_path(&store), b"user-updated-binary").expect("seed updated exe");
    ytdlp::ensure_from_bundle(&store, &[bundled]);
    assert_eq!(
        std::fs::read(ytdlp::exe_path(&store)).expect("read exe"),
        b"user-updated-binary",
        "用户更新过的版本不得被内置旧版顶掉"
    );
}
