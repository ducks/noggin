//! Gemini CLI subprocess invocation
//!
//! Invokes the `@google/gemini-cli` via npx as a subprocess.
//! Gemini provides deep security audits and thorough multi-file analysis.

use crate::error::{Error, LlmError};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tracing::debug;

/// Gemini CLI client
#[derive(Debug, Clone)]
pub struct GeminiClient {
    /// Timeout for subprocess execution (default: 300s / 5 minutes)
    pub timeout_secs: u64,
}

impl GeminiClient {
    /// Create a new Gemini client with default configuration
    pub fn new() -> Self {
        Self { timeout_secs: 300 }
    }

    /// Query Gemini CLI and return the response
    pub async fn query(&self, prompt: &str) -> Result<String, Error> {
        // Build command: npx @google/gemini-cli "prompt"
        let mut cmd = Command::new("npx");
        cmd.args(["@google/gemini-cli", prompt])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        debug!(
            "Executing: npx @google/gemini-cli [prompt: {} chars]",
            prompt.len()
        );

        // Execute with timeout
        let timeout_duration = Duration::from_secs(self.timeout_secs);
        let child = cmd.spawn().map_err(|e| {
            Error::Llm(LlmError::RequestFailed {
                model: "gemini".to_string(),
                source: format!("Failed to spawn process: {}", e),
            })
        })?;

        let output = tokio::time::timeout(timeout_duration, child.wait_with_output())
            .await
            .map_err(|_| Error::Llm(LlmError::RequestFailed {
                model: "gemini".to_string(),
                source: format!("Timeout after {}s", self.timeout_secs),
            }))?
            .map_err(|e| Error::Llm(LlmError::RequestFailed {
                model: "gemini".to_string(),
                source: format!("Process error: {}", e),
            }))?;

        // Check exit code
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Llm(LlmError::RequestFailed {
                model: "gemini".to_string(),
                source: stderr.to_string(),
            }));
        }

        // Get response from stdout (plain text)
        let stdout = String::from_utf8(output.stdout).map_err(|e| {
            Error::Llm(LlmError::InvalidResponse {
                model: "gemini".to_string(),
                details: format!("Invalid UTF-8 in stdout: {}", e),
            })
        })?;

        debug!("Gemini query completed successfully");
        Ok(stdout)
    }
}

impl Default for GeminiClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl crate::llm::LLMProvider for GeminiClient {
    async fn query(&self, prompt: &str) -> Result<String, Error> {
        self.query(prompt).await
    }

    fn name(&self) -> &str {
        "gemini"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let client = GeminiClient::new();
        assert_eq!(client.timeout_secs, 300);
    }
}
