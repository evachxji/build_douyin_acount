use std::process::Command;
use std::sync::RwLock;

use crate::domain::error::DomainError;

/// 内置工具目录（storage/tools，lib.rs setup 与 AppState 构造时登记）。
static TOOLS_DIR: RwLock<Option<std::path::PathBuf>> = RwLock::new(None);

/// 登记内置工具目录；可重复调用（测试用 tempdir 各自覆盖，无 exe 时自然落回 PATH）。
pub fn set_tools_dir(dir: std::path::PathBuf) {
    *TOOLS_DIR.write().expect("tools dir lock poisoned") = Some(dir);
}

/// 工具路径解析：环境变量覆盖（FFMPEG_PATH / FFPROBE_PATH，e2e/开发逃生口）
/// → 内置 exe（storage/tools/<name>.exe，存在才用）→ PATH 裸命令。
fn resolve_tool(name: &str) -> String {
    let env_name = format!("{name}_PATH").to_uppercase();
    if let Ok(p) = std::env::var(&env_name) {
        if !p.is_empty() {
            return p;
        }
    }
    if let Some(dir) = TOOLS_DIR.read().expect("tools dir lock poisoned").as_ref() {
        let exe = dir.join(format!("{name}.exe"));
        if exe.is_file() {
            return exe.to_string_lossy().into_owned();
        }
    }
    name.to_string()
}

/// 执行外部工具；路径解析见 `resolve_tool`。
pub fn run_tool(name: &str, args: &[&str]) -> Result<Vec<u8>, DomainError> {
    let tool = resolve_tool(name);
    let output = Command::new(&tool).args(args).output().map_err(|e| DomainError::Process {
        cmd: tool.clone(),
        detail: format!("spawn failed: {e}"),
    })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(DomainError::Process {
            cmd: tool,
            detail,
        });
    }
    Ok(output.stdout)
}

/// 将本地文件编码为 base64（供 Ollama Vision 使用）。
pub fn file_to_base64(path: &std::path::Path) -> Result<String, DomainError> {
    let bytes = std::fs::read(path)?;
    Ok(base64_encode(&bytes))
}

/// 极简 base64 编码（避免额外依赖；仅用于图片数据）。
pub fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(n >> 6) as usize & 63] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[n as usize & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64_encode;

    #[test]
    fn base64_roundtrip_known_values() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }
}
