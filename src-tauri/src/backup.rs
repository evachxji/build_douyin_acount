//! 备份导出/导入：把全部用户数据（视频、封面、数据库快照、采集 cookie）打成单个 zip。
//! 导入为覆盖式、两段执行：运行中解压校验到 `.import_staging/` 并写 `.import_pending` 标记；
//! 重启后在打开数据库之前（进程无任何文件句柄）由 `apply_pending_import` 完成替换。

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::db::Database;
use crate::domain::error::DomainError;
use crate::media::MediaStore;

const MANIFEST_FORMAT: &str = "douyin-backup";
const STAGING_DIR: &str = ".import_staging";
const PENDING_MARK: &str = ".import_pending";

#[derive(Debug, Serialize)]
pub struct ExportStats {
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct ImportPreview {
    pub assets: i64,
    pub videos: u64,
    pub bytes: u64,
}

fn zip_err(e: zip::result::ZipError) -> DomainError {
    DomainError::InvalidData(format!("备份压缩包错误: {e}"))
}

/// 导出全部用户数据为一个 zip：manifest + db 快照 + videos/ + covers/ + cookies.txt。
/// 视频已是压缩格式（mp4），用 Stored 直存省 CPU；db/文本用 deflate。
pub fn export_backup(
    store: &MediaStore,
    db: &Database,
    dest: &Path,
) -> Result<ExportStats, DomainError> {
    // 一致性快照：WAL 模式下直接拷 app.db 会丢未落盘数据，VACUUM INTO 生成完整单文件。
    let snapshot = store.root().join("tmp").join("backup_snapshot.db");
    fs::create_dir_all(snapshot.parent().expect("tmp parent"))?;
    if snapshot.exists() {
        fs::remove_file(&snapshot)?;
    }
    {
        let conn = db.conn();
        let sql = format!(
            "VACUUM INTO '{}'",
            snapshot.to_string_lossy().replace('\'', "''")
        );
        conn.execute_batch(&sql)?;
    }

    let result = write_zip(store, db, &snapshot, dest);
    let _ = fs::remove_file(&snapshot);
    let files = result?;
    let bytes = fs::metadata(dest)?.len();
    Ok(ExportStats { files, bytes })
}

fn write_zip(
    store: &MediaStore,
    db: &Database,
    snapshot: &Path,
    dest: &Path,
) -> Result<u64, DomainError> {
    let mut zw = ZipWriter::new(fs::File::create(dest)?);
    let deflate = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut files: u64 = 0;

    let (assets, tasks) = {
        let conn = db.conn();
        let a: i64 = conn.query_row("SELECT COUNT(*) FROM video_assets", [], |r| r.get(0))?;
        let t: i64 = conn.query_row("SELECT COUNT(*) FROM agent_tasks", [], |r| r.get(0))?;
        (a, t)
    };
    let manifest = serde_json::json!({
        "format": MANIFEST_FORMAT,
        "version": 1,
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "counts": { "assets": assets, "tasks": tasks },
    });
    zw.start_file("manifest.json", deflate).map_err(zip_err)?;
    zw.write_all(manifest.to_string().as_bytes())?;
    files += 1;

    let mut add_file = |zip_name: String, path: &Path, opts: SimpleFileOptions| -> Result<(), DomainError> {
        zw.start_file(zip_name, opts).map_err(zip_err)?;
        std::io::copy(&mut fs::File::open(path)?, &mut zw)?;
        files += 1;
        Ok(())
    };

    add_file("db/app.db".to_string(), snapshot, deflate)?;
    // videos / covers 是平铺目录（uuid.mp4 / uuid.jpg），逐文件流式写入。
    for (dir, prefix, opts) in [
        (store.videos_dir(), "videos", stored),
        (store.covers_dir(), "covers", deflate),
    ] {
        if !dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name().expect("is_file has name").to_string_lossy();
            add_file(format!("{prefix}/{name}"), &path, opts)?;
        }
    }
    let cookies = store.root().join("cookies.txt");
    if cookies.is_file() {
        add_file("cookies.txt".to_string(), &cookies, deflate)?;
    }

    zw.finish().map_err(zip_err)?;
    Ok(files)
}

/// 导入第一步：校验 zip 并解压到暂存区，返回包内统计。
/// 不写 pending 标记——用户在前端确认覆盖后由 `mark_import_pending` 单独标记，
/// 否则用户取消导入也会在下次重启时被误应用。
pub fn stage_import(store: &MediaStore, src: &Path) -> Result<ImportPreview, DomainError> {
    let staging = store.root().join(STAGING_DIR);
    let result = stage_import_inner(src, &staging);
    if result.is_err() {
        // 校验/解压失败：清掉暂存，不留垃圾。
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

/// 导入确认：暂存区就绪后写 pending 标记，重启时应用（覆盖现有数据）。
pub fn mark_import_pending(store: &MediaStore) -> Result<(), DomainError> {
    let staging = store.root().join(STAGING_DIR);
    if !staging.join("db").join("app.db").is_file() {
        return Err(DomainError::InvalidData(
            "没有待导入的备份（请先选择备份包）".to_string(),
        ));
    }
    fs::write(store.root().join(PENDING_MARK), "pending")?;
    Ok(())
}

fn stage_import_inner(src: &Path, staging: &Path) -> Result<ImportPreview, DomainError> {
    let mut archive = ZipArchive::new(fs::File::open(src)?).map_err(zip_err)?;

    // 身份校验：无 manifest 或 format 不符 → 不是本应用的备份包。
    let manifest_raw = {
        let mut mf = archive.by_name("manifest.json").map_err(|_| {
            DomainError::InvalidData("不是本应用的备份包（缺少 manifest.json）".to_string())
        })?;
        let mut s = String::new();
        mf.read_to_string(&mut s)?;
        s
    };
    let manifest: serde_json::Value = serde_json::from_str(&manifest_raw)?;
    if manifest["format"].as_str() != Some(MANIFEST_FORMAT) {
        return Err(DomainError::InvalidData(
            "不是本应用的备份包（format 不匹配）".to_string(),
        ));
    }

    if staging.exists() {
        fs::remove_dir_all(staging)?;
    }
    fs::create_dir_all(staging)?;

    // 只落地已知结构（enclosed_name 防 Zip Slip 路径穿越），未知条目跳过。
    let mut videos: u64 = 0;
    let mut bytes: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(zip_err)?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let name = name.to_string_lossy().replace('\\', "/");
        let known = name == "db/app.db"
            || name == "cookies.txt"
            || name.starts_with("videos/")
            || name.starts_with("covers/");
        if !known || entry.is_dir() {
            continue;
        }
        let out = staging.join(&name);
        fs::create_dir_all(out.parent().expect("entry parent"))?;
        let mut dst = fs::File::create(&out)?;
        bytes += std::io::copy(&mut entry, &mut dst)?;
        if name.starts_with("videos/") {
            videos += 1;
        }
    }

    // 校验包内数据库可读且结构完整（只读打开，不做迁移，避免改动备份）。
    let staged_db = staging.join("db").join("app.db");
    if !staged_db.is_file() {
        return Err(DomainError::InvalidData(
            "备份包缺少数据库（db/app.db）".to_string(),
        ));
    }
    let conn = rusqlite::Connection::open_with_flags(
        &staged_db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let assets = conn
        .query_row("SELECT COUNT(*) FROM video_assets", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(|e| DomainError::InvalidData(format!("备份包数据库不可读: {e}")))?;
    drop(conn);

    Ok(ImportPreview {
        assets,
        videos,
        bytes,
    })
}

/// 导入第二步：重启后、打开数据库之前调用。无标记时立即返回 Ok(false)；
/// 有标记则把暂存内容覆盖式搬入 storage（先改名旧物再搬入，失败回滚）。
/// 成功才清理标记与暂存；失败保留二者，下次启动自动重试。
pub fn apply_pending_import(store: &MediaStore) -> Result<bool, DomainError> {
    let mark = store.root().join(PENDING_MARK);
    let staging = store.root().join(STAGING_DIR);
    if !mark.exists() {
        return Ok(false);
    }
    let result = apply_staged(store, &staging);
    match &result {
        Ok(()) => {
            let _ = fs::remove_dir_all(&staging);
            let _ = fs::remove_file(&mark);
        }
        Err(e) => {
            let _ = fs::create_dir_all(store.logs_dir());
            let _ = fs::write(
                store.logs_dir().join("import_error.txt"),
                format!("导入备份应用失败（已回滚到原数据，将在下次启动重试）: {e}"),
            );
        }
    }
    result.map(|_| true)
}

struct Swap {
    target: PathBuf,
    staged: PathBuf,
    is_dir: bool,
}

/// 重启竞争：`app.restart()` 先启动新进程、旧进程后退出，旧进程持有的文件句柄
/// （SQLite 连接、被 WebView2 打开的视频/封面）会让 rename 报共享冲突——
/// 等待重试，旧进程退出后句柄释放即成功（最多等 5 秒）。
fn rename_with_retry(from: &Path, to: &Path) -> Result<(), DomainError> {
    let mut waited_ms = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if is_file_in_use(&e) && waited_ms < 5000 => {
                std::thread::sleep(std::time::Duration::from_millis(100));
                waited_ms += 100;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

fn is_file_in_use(e: &std::io::Error) -> bool {
    // 32 = ERROR_SHARING_VIOLATION；5 = ERROR_ACCESS_DENIED（被占用文件两种都可能报）。
    matches!(e.raw_os_error(), Some(32) | Some(5))
}

fn old_path(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".old");
    PathBuf::from(s)
}

fn remove_any(p: &Path) -> Result<(), DomainError> {
    if p.is_dir() {
        fs::remove_dir_all(p)?;
    } else if p.exists() {
        fs::remove_file(p)?;
    }
    Ok(())
}

fn rollback(swaps: &[Swap]) {
    for s in swaps {
        let old = old_path(&s.target);
        if s.target.exists() {
            // 已搬入的新物搬回暂存（保持暂存完整，供下次启动重试），而不是删除。
            let _ = rename_with_retry(&s.target, &s.staged);
        }
        if old.exists() {
            let _ = rename_with_retry(&old, &s.target);
        }
    }
}

/// 覆盖式替换：videos/covers 整目录替换（恢复到 zip 状态，多余文件被清掉）；
/// app.db 单文件替换并清理旧 WAL 伴生；cookies.txt 包里有则替换、无则删除。
/// 全部先改名 .old 再搬入（Windows rename 不覆盖已存在目标），任一步失败回滚。
fn apply_staged(store: &MediaStore, staging: &Path) -> Result<(), DomainError> {
    let swaps = [
        Swap {
            target: store.videos_dir(),
            staged: staging.join("videos"),
            is_dir: true,
        },
        Swap {
            target: store.covers_dir(),
            staged: staging.join("covers"),
            is_dir: true,
        },
        Swap {
            target: store.db_dir().join("app.db"),
            staged: staging.join("db").join("app.db"),
            is_dir: false,
        },
        Swap {
            target: store.root().join("cookies.txt"),
            staged: staging.join("cookies.txt"),
            is_dir: false,
        },
    ];
    // 阶段1：旧物全部改名 .old。
    for (i, s) in swaps.iter().enumerate() {
        let old = old_path(&s.target);
        if old.exists() {
            remove_any(&old)?;
        }
        if s.target.exists() {
            if let Err(e) = rename_with_retry(&s.target, &old) {
                rollback(&swaps[..i]);
                return Err(e);
            }
        }
    }
    // 阶段2：搬入新物；包内没有的目录置空、没有的文件保持不存在（= 覆盖删除）。
    if let Err(e) = (|| -> Result<(), DomainError> {
        for s in &swaps {
            if s.staged.exists() {
                rename_with_retry(&s.staged, &s.target)?;
            } else if s.is_dir {
                fs::create_dir_all(&s.target)?;
            }
        }
        Ok(())
    })() {
        rollback(&swaps);
        return Err(e);
    }
    // 阶段3：删 .old 与旧库 WAL 伴生（属于旧库，留着会污染新库）。
    for s in &swaps {
        let _ = remove_any(&old_path(&s.target));
    }
    let db = store.db_dir().join("app.db");
    for suffix in ["-wal", "-shm"] {
        let mut x = db.as_os_str().to_os_string();
        x.push(suffix);
        let _ = fs::remove_file(PathBuf::from(x));
    }
    Ok(())
}
