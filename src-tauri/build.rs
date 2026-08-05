fn main() {
    tauri_build::build();

    // Windows MSVC：为测试目标嵌入 Common Controls v6 manifest，
    // 避免 Tauri mock runtime 测试二进制启动即崩溃（0xc0000139）。
    #[cfg(all(windows, target_env = "msvc"))]
    {
        println!("cargo:rerun-if-changed=windows-app-manifest.xml");
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("windows-app-manifest.xml");
        // LNK1220: /MANIFESTINPUT 需要同时声明 /MANIFEST:EMBED
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
