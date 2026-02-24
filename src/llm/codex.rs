//! Codex CLI subprocess invocation with JSON parsing
//!
//! Invokes the `codex` CLI (gpt-5.2-codex) as a subprocess with JSON output mode.
//! Codex writes JSON to stderr instead of stdout.

use crate::error::{Error, LlmError};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tracing::debug;

/// Codex CLI client
#[derive(Debug, Clone)]
pub struct CodexClient {
    /// Timeout for subprocess execution (default: 120s)
    pub timeout_secs: u64,
}

impl CodexClient {
    /// Create a new Codex client with default configuration
    pub fn new() -> Self {
        Self { timeout_secs: 120 }
    }

    /// Query Codex CLI and return the response
    pub async fn query(&self, prompt: &str) -> Result<String, Error> {
        // Build command: codex exec --json -s read-only "prompt"
        let mut cmd = Command::new("codex");
        cmd.args(["exec", "--json", "-s", "read-only", prompt])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        debug!(
            "Executing: codex exec --json -s read-only [prompt: {} chars]",
            prompt.len()
        );

        // Execute with timeout
        let timeout_duration = Duration::from_secs(self.timeout_secs);
        let child = cmd.spawn().map_err(|e| {
            Error::Llm(LlmError::RequestFailed {
                model: "codex".to_string(),
                source: format!("Failed to spawn process: {}", e),
            })
        })?;

        let output = tokio::time::timeout(timeout_duration, child.wait_with_output())
            .await
            .map_err(|_| Error::Llm(LlmError::RequestFailed {
                model: "codex".to_string(),
                source: format!("Timeout after {}s", self.timeout_secs),
            }))??
            .map_err(|e| Error::Llm(LlmError::RequestFailed {
                model: "codex".to_string(),
                source: format!("Process error: {}", e),
            }))?;

        // Check exit code
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Llm(LlmError::RequestFailed {
                model: "codex".to_string(),
                source: stderr.to_string(),
            }));
        }

        // Parse JSON response from stderr (codex writes to stderr)
        let stderr = String::from_utf8(output.stderr).map_err(|e| {
            Error::Llm(LlmError::InvalidResponse {
                model: "codex".to_string(),
                details: format!("Invalid UTF-8 in stderr: {}", e),
            })
        })?;

        let response: CodexResponse = serde_json::from_str(&stderr).map_err(|e| {
            Error::Llm(LlmError::InvalidResponse {
                model: "codex".to_string(),
                details: format!(
                    "Failed to parse JSON: {}. Stderr: {}",
                    e,
                    stderr.chars().take(200).collect::<String>()
                ),
            })
        })?;

        debug!("Codex query completed successfully");
        Ok(response.agent_message)
    }
}

impl Default for CodexClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Response from Codex CLI (JSON format)
#[derive(Debug, Deserialize, Serialize)]
pub struct CodexResponse {
    /// The agent's response text
    pub agent_message: String,
}

#[async_trait::async_trait]
impl crate::llm::LLMProvider for CodexClient {
    async fn query(&self, prompt: &str) -> Result<String, Error> {
        self.query(prompt).await
    }

    fn name(&self) -> &str {
        "codex"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_codex_response() {
        let json = r#"{"agent_message": "Hello from Codex"}"#;
        let response: CodexResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.agent_message, "Hello from Codex");
    }

    #[test]
    fn test_config_defaults() {
        let client = CodexClient::new();
        assert_eq!(client.timeout_secs, 120);
    }
}
