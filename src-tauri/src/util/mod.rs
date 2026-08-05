use std::process::Command;

use crate::domain::error::DomainError;

/// 执行外部工具；支持通过环境变量覆盖路径（如 FFMPEG_PATH / FFPROBE_PATH）。
pub fn run_tool(name: &str, args: &[&str]) -> Result<Vec<u8>, DomainError> {
    let env_name = format!("{name}_PATH").to_uppercase();
    let tool = std::env::var(&env_name).unwrap_or_else(|_| name.to_string());
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
