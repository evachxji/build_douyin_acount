//! 内置外部工具管理（yt-dlp、FFmpeg，随安装包分发并支持应用内一键更新）。

pub mod ytdlp {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use crate::domain::error::DomainError;
    use crate::media::MediaStore;

    const EXE_NAME: &str = "yt-dlp.exe";
    const RELEASE_API: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
    const DOWNLOAD_URL: &str =
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
    const USER_AGENT: &str = "build_douyin_acount";

    /// 内置 yt-dlp 落点：数据目录 tools/ 下（可写；备份不含它，丢了重新下载即可）。
    pub fn exe_path(store: &MediaStore) -> PathBuf {
        store.tools_dir().join(EXE_NAME)
    }

    /// 当前版本：`yt-dlp.exe --version` stdout 首行（PyInstaller 冷启动 1-3s，勿高频调用）。
    pub fn current_version(exe: &Path) -> Option<String> {
        let output = std::process::Command::new(exe)
            .arg("--version")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let v = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()?
            .trim()
            .to_string();
        if v.is_empty() {
            None
        } else {
            Some(v)
        }
    }

    /// `current_version` 的缓存版：exe 路径+mtime 不变直接命中（设置页每次打开都调，
    /// 命中后毫秒级；exe 被一键更新替换后 mtime 变化，自动重读）。
    pub fn current_version_cached(exe: &Path) -> Option<String> {
        use std::sync::Mutex;
        use std::time::SystemTime;
        type VersionCache = Mutex<Option<(PathBuf, Option<SystemTime>, Option<String>)>>;
        static CACHE: VersionCache = Mutex::new(None);
        let mtime = std::fs::metadata(exe).and_then(|m| m.modified()).ok();
        {
            let guard = CACHE.lock().expect("ytdlp version cache lock poisoned");
            if let Some((p, t, v)) = guard.as_ref() {
                if p == exe && *t == mtime {
                    return v.clone();
                }
            }
        }
        let v = current_version(exe);
        let mut guard = CACHE.lock().expect("ytdlp version cache lock poisoned");
        *guard = Some((exe.to_path_buf(), mtime, v.clone()));
        v
    }

    /// 安装版打包的 yt-dlp.exe 释放到数据目录：**仅当目标缺失**——绝不覆盖，
    /// 用户一键更新过的新版不能被内置旧版顶掉。失败仅 warn，设置页可手动下载兜底。
    pub fn ensure_from_bundle(store: &MediaStore, candidates: &[PathBuf]) {
        let dest = exe_path(store);
        if dest.exists() {
            return;
        }
        let Some(src) = candidates.iter().find(|p| p.exists()) else {
            return;
        };
        if let Err(e) = std::fs::copy(src, &dest) {
            tracing::warn!("释放内置 yt-dlp 失败（可到设置页手动下载）: {e}");
        }
    }

    /// reqwest 客户端：带 UA（GitHub API 强制）；**不配 no_proxy**，走系统代理
    /// （采集 TikTok/IG 本就需要梯子，GitHub 直连困难时代理能救）。
    pub fn http_client(timeout: Duration) -> Result<reqwest::Client, DomainError> {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(timeout)
            .build()
            .map_err(http_err)
    }

    /// 最新版本号（GitHub releases API 的 tag_name，如 `2025.08.22`）。
    pub async fn fetch_latest_version(client: &reqwest::Client) -> Result<String, DomainError> {
        let v: serde_json::Value = client
            .get(RELEASE_API)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(http_err)?
            .json()
            .await
            .map_err(http_err)?;
        v["tag_name"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| DomainError::InvalidData("GitHub release 响应缺 tag_name".to_string()))
    }

    /// 流式下载最新 exe 到 dest（调用方传 `.new` 临时路径，验证通过后再 install）。
    /// `on_progress(downloaded, total)` 每 chunk 回调一次（total 取 Content-Length，未知为 0）。
    pub async fn download_latest<F: FnMut(u64, u64)>(
        client: &reqwest::Client,
        dest: &Path,
        mut on_progress: F,
    ) -> Result<(), DomainError> {
        use std::io::Write;

        let mut resp = client
            .get(DOWNLOAD_URL)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(http_err)?;
        let total = resp.content_length().unwrap_or(0);
        let mut file = std::fs::File::create(dest)?;
        let mut downloaded: u64 = 0;
        while let Some(chunk) = resp.chunk().await.map_err(http_err)? {
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            on_progress(downloaded, total);
        }
        // 真实 exe ~17MB；拿到几 KB 说明是错误页/截断，不能留在磁盘。
        if downloaded < 1_000_000 {
            return Err(DomainError::InvalidData(format!(
                "yt-dlp 下载内容异常（{downloaded} 字节）"
            )));
        }
        Ok(())
    }

    /// 用下载好的新 exe 替换内置 exe。Windows 下运行中的 exe 可 rename 不可覆盖：
    /// exe→.old、new→exe；.old 删不掉（采集进程占用中）不致命，下次 install 前再清。
    pub fn install(store: &MediaStore, new_file: &Path) -> Result<(), DomainError> {
        let exe = exe_path(store);
        let old = exe.with_extension("exe.old");
        if old.exists() {
            let _ = std::fs::remove_file(&old);
        }
        if exe.exists() {
            std::fs::rename(&exe, &old)?;
        }
        std::fs::rename(new_file, &exe)?;
        let _ = std::fs::remove_file(&old);
        Ok(())
    }

    /// 版本分段数值化（calver `YYYY.MM.DD`，兼容无前导零写法 `2025.8.1`——
    /// 字符串比较会把 "2025.8.1" 误判大于 "2025.08.22"）。
    pub fn version_key(v: &str) -> Vec<u64> {
        v.split('.').map(|s| s.trim().parse().unwrap_or(0)).collect()
    }

    pub fn update_available(current: &str, latest: &str) -> bool {
        version_key(latest) > version_key(current)
    }

    fn http_err(e: reqwest::Error) -> DomainError {
        DomainError::Http {
            status: e.status().map(|s| s.as_u16()).unwrap_or(0),
            body: e.to_string(),
        }
    }
}

/// 内置 FFmpeg（ffmpeg.exe + ffprobe.exe），源用 gyan.dev release 构建
/// （ffmpeg.org 官方推荐的 Windows 构建，版本号是干净 semver；
/// BtbN GitHub 构建包大近一倍且版本串是 git N-revision 不好比较，弃选）。
pub mod ffmpeg {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use crate::domain::error::DomainError;
    use crate::media::MediaStore;

    const EXE_NAME: &str = "ffmpeg.exe";
    const FFPROBE_NAME: &str = "ffprobe.exe";
    /// 最新版本号（纯文本 semver，如 `9.0.1`）。
    const VERSION_URL: &str = "https://www.gyan.dev/ffmpeg/builds/release-version";
    /// essentials 构建 zip（~106MB，内含 bin/ffmpeg.exe、bin/ffprobe.exe、bin/ffplay.exe，只取前两个）。
    const DOWNLOAD_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
    const USER_AGENT: &str = "build_douyin_acount";
    /// 下载体积下限：真 zip ~106MB，远小于此说明是错误页/截断。
    const MIN_ZIP_BYTES: u64 = 50_000_000;

    /// 内置 ffmpeg 落点：数据目录 tools/ 下（与 yt-dlp 同，备份不含，丢了重新下载）。
    pub fn exe_path(store: &MediaStore) -> PathBuf {
        store.tools_dir().join(EXE_NAME)
    }

    pub fn ffprobe_path(store: &MediaStore) -> PathBuf {
        store.tools_dir().join(FFPROBE_NAME)
    }

    /// 从 `-version` 首行解析版本号：`ffmpeg version 9.0.1-essentials_build-www.gyan.dev ...`
    /// 取 "version " 后首个 token、再取 `-` 前的数字段；非数字开头（如 BtbN 的 `N-xxxxx`）视为不可解析。
    pub fn parse_version_output(line: &str) -> Option<String> {
        let token = line.split_whitespace().nth(2)?;
        let ver = token.split('-').next()?.trim();
        if ver.is_empty() || !ver.chars().next()?.is_ascii_digit() {
            return None;
        }
        Some(ver.to_string())
    }

    /// 当前版本：`ffmpeg.exe -version` stdout 首行解析（冷启动数百 ms，勿高频调用）。
    pub fn current_version(exe: &Path) -> Option<String> {
        let output = std::process::Command::new(exe).arg("-version").output().ok()?;
        if !output.status.success() {
            return None;
        }
        let line = String::from_utf8_lossy(&output.stdout);
        parse_version_output(line.lines().next()?)
    }

    /// `current_version` 的缓存版：exe 路径+mtime 不变直接命中（同 yt-dlp 缓存模式）。
    pub fn current_version_cached(exe: &Path) -> Option<String> {
        use std::sync::Mutex;
        use std::time::SystemTime;
        type VersionCache = Mutex<Option<(PathBuf, Option<SystemTime>, Option<String>)>>;
        static CACHE: VersionCache = Mutex::new(None);
        let mtime = std::fs::metadata(exe).and_then(|m| m.modified()).ok();
        {
            let guard = CACHE.lock().expect("ffmpeg version cache lock poisoned");
            if let Some((p, t, v)) = guard.as_ref() {
                if p == exe && *t == mtime {
                    return v.clone();
                }
            }
        }
        let v = current_version(exe);
        let mut guard = CACHE.lock().expect("ffmpeg version cache lock poisoned");
        *guard = Some((exe.to_path_buf(), mtime, v.clone()));
        v
    }

    /// 安装版打包的 ffmpeg/ffprobe 释放到数据目录：**仅当目标缺失**——绝不覆盖，
    /// 用户一键更新过的新版不能被内置旧版顶掉。失败仅 warn，设置页可手动下载兜底。
    /// `candidates` 是候选目录（打包 resource_dir、开发期 CARGO_MANIFEST_DIR/resources）。
    pub fn ensure_from_bundle(store: &MediaStore, candidates: &[PathBuf]) {
        for name in [EXE_NAME, FFPROBE_NAME] {
            let dest = store.tools_dir().join(name);
            if dest.exists() {
                continue;
            }
            let Some(src) = candidates.iter().map(|d| d.join(name)).find(|p| p.exists()) else {
                continue;
            };
            if let Err(e) = std::fs::copy(&src, &dest) {
                tracing::warn!("释放内置 {name} 失败（可到设置页手动下载）: {e}");
            }
        }
    }

    /// reqwest 客户端：带 UA；**不配 no_proxy**，走系统代理（同 yt-dlp 契约）。
    pub fn http_client(timeout: Duration) -> Result<reqwest::Client, DomainError> {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(timeout)
            .build()
            .map_err(http_err)
    }

    /// 最新版本号（release-version 纯文本，如 `9.0.1`）。
    pub async fn fetch_latest_version(client: &reqwest::Client) -> Result<String, DomainError> {
        let text = client
            .get(VERSION_URL)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(http_err)?
            .text()
            .await
            .map_err(http_err)?;
        let v = text.trim();
        if v.is_empty() || !v.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return Err(DomainError::InvalidData(format!(
                "gyan.dev release-version 响应异常: {v:?}"
            )));
        }
        Ok(v.to_string())
    }

    /// 流式下载 essentials zip 到 dest（调用方传 `.zip.new` 临时路径，验证通过后再 install）。
    pub async fn download_latest<F: FnMut(u64, u64)>(
        client: &reqwest::Client,
        dest: &Path,
        mut on_progress: F,
    ) -> Result<(), DomainError> {
        use std::io::Write;

        let mut resp = client
            .get(DOWNLOAD_URL)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(http_err)?;
        let total = resp.content_length().unwrap_or(0);
        let mut file = std::fs::File::create(dest)?;
        let mut downloaded: u64 = 0;
        while let Some(chunk) = resp.chunk().await.map_err(http_err)? {
            file.write_all(&chunk)?;
            downloaded += chunk.len() as u64;
            on_progress(downloaded, total);
        }
        if downloaded < MIN_ZIP_BYTES {
            return Err(DomainError::InvalidData(format!(
                "FFmpeg 下载内容异常（{downloaded} 字节）"
            )));
        }
        Ok(())
    }

    /// 安装 = 解压 + 校验 + 轮换。任一失败不留下半成品：先全部校验通过才轮换，
    /// 失败清理 .new（旧 exe 原样保留，可重试）。
    pub fn install(store: &MediaStore, zip_file: &Path) -> Result<(), DomainError> {
        let extracted = extract_zipped_exes(store, zip_file)?;
        // 下载件必须真能跑（截断/架构不符在这里现形）。
        for (_, new_path) in &extracted {
            if current_version(new_path).is_none() {
                for (_, p) in &extracted {
                    let _ = std::fs::remove_file(p);
                }
                return Err(DomainError::InvalidData(format!(
                    "下载的 {} 无法运行（-version 无输出）",
                    new_path.display()
                )));
            }
        }
        rotate_install(&extracted)
    }

    /// 从 zip 解压 bin/ffmpeg.exe、bin/ffprobe.exe 为 .new 临时文件。
    /// 返回 (exe 落点, .new 临时文件) 列表；失败清理已解压的半成品。
    pub fn extract_zipped_exes(
        store: &MediaStore,
        zip_file: &Path,
    ) -> Result<Vec<(PathBuf, PathBuf)>, DomainError> {
        let archive_file = std::fs::File::open(zip_file)?;
        let mut archive = zip::ZipArchive::new(archive_file)
            .map_err(|e| DomainError::InvalidData(format!("FFmpeg 压缩包无法打开: {e}")))?;
        let mut extracted: Vec<(PathBuf, PathBuf)> = Vec::new();
        let result: Result<(), DomainError> = (|| {
            for name in [EXE_NAME, FFPROBE_NAME] {
                let bin_suffix = format!("/bin/{name}");
                let mut found = false;
                for i in 0..archive.len() {
                    let mut entry = archive.by_index(i).map_err(|e| {
                        DomainError::InvalidData(format!("FFmpeg 压缩包读取失败: {e}"))
                    })?;
                    let entry_name = entry.name().replace('\\', "/");
                    if !entry_name.ends_with(&bin_suffix) {
                        continue;
                    }
                    let exe = store.tools_dir().join(name);
                    let new_path = exe.with_extension("exe.new");
                    let mut out = std::fs::File::create(&new_path)?;
                    std::io::copy(&mut entry, &mut out)?;
                    extracted.push((exe, new_path));
                    found = true;
                    break;
                }
                if !found {
                    return Err(DomainError::InvalidData(format!(
                        "FFmpeg 压缩包内未找到 bin/{name}"
                    )));
                }
            }
            Ok(())
        })();
        if let Err(e) = result {
            for (_, new_path) in &extracted {
                let _ = std::fs::remove_file(new_path);
            }
            return Err(e);
        }
        Ok(extracted)
    }

    /// exe→.old、new→exe 轮换（Windows 运行中的 exe 可 rename 不可覆盖；
    /// .old 删不掉不致命，下次 install 前再清）。
    pub fn rotate_install(extracted: &[(PathBuf, PathBuf)]) -> Result<(), DomainError> {
        for (exe, new_path) in extracted {
            let old = exe.with_extension("exe.old");
            if old.exists() {
                let _ = std::fs::remove_file(&old);
            }
            if exe.exists() {
                std::fs::rename(exe, &old)?;
            }
            std::fs::rename(new_path, exe)?;
            let _ = std::fs::remove_file(&old);
        }
        Ok(())
    }

    /// 版本分段数值化（semver，同 yt-dlp 契约：字符串比较会误判）。
    pub fn version_key(v: &str) -> Vec<u64> {
        v.split('.').map(|s| s.trim().parse().unwrap_or(0)).collect()
    }

    pub fn update_available(current: &str, latest: &str) -> bool {
        version_key(latest) > version_key(current)
    }

    fn http_err(e: reqwest::Error) -> DomainError {
        DomainError::Http {
            status: e.status().map(|s| s.as_u16()).unwrap_or(0),
            body: e.to_string(),
        }
    }
}
