use std::path::{Path, PathBuf};

use crate::domain::error::DomainError;
use crate::util::run_tool;

/// ASR 转写结果。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub language: String,
}

/// 音频转写能力抽象（便于测试注入桩实现）。
pub trait Asr: Send + Sync {
    fn transcribe(&self, wav: &Path) -> Result<Transcript, DomainError>;
}

/// Whisper 实现：通过 Python sidecar（faster-whisper）完成转写。
pub struct WhisperAsr {
    pub python: String,
    pub script: PathBuf,
    pub model: String,
}

impl WhisperAsr {
    pub fn new(script: impl Into<PathBuf>, model: impl Into<String>) -> Self {
        Self {
            python: std::env::var("PYTHON_PATH").unwrap_or_else(|_| "python".to_string()),
            script: script.into(),
            model: model.into(),
        }
    }

    /// 覆盖 Python 解释器路径（GUI 设置中的 whisper_python）。
    pub fn with_python(mut self, python: impl Into<String>) -> Self {
        self.python = python.into();
        self
    }
}

impl Asr for WhisperAsr {
    fn transcribe(&self, wav: &Path) -> Result<Transcript, DomainError> {
        let out = run_tool(
            &self.python,
            &[
                self.script.to_string_lossy().as_ref(),
                wav.to_string_lossy().as_ref(),
                &self.model,
            ],
        )?;
        let value: serde_json::Value = serde_json::from_slice(&out)
            .map_err(|e| DomainError::InvalidData(format!("whisper output invalid json: {e}")))?;
        Ok(Transcript {
            text: value["text"].as_str().unwrap_or_default().to_string(),
            language: value["language"].as_str().unwrap_or("unknown").to_string(),
        })
    }
}
