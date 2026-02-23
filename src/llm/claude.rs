//! Claude CLI subprocess invocation with JSON parsing
//!
//! Invokes the `claude` CLI as a subprocess with JSON output mode,
//! handles timeouts, rate limits, and provides retry logic.

use crate::error::{Error, LlmError};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::{debug, warn};

/// Configuration for Claude CLI client
#[derive(Debug, Clone)]
pub struct ClaudeConfig {
    /// Timeout for subprocess execution (default: 30s)
    pub timeout_secs: u64,
    /// Maximum retry attempts (default: 3)
    pub max_retries: u32,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            max_retries: 3,
        }
    }
}

/// Claude CLI client
pub struct ClaudeClient {
    config: ClaudeConfig,
}

impl ClaudeClient {
    /// Create a new Claude client with default configuration
    pub fn new() -> Self {
        Self {
            config: ClaudeConfig::default(),
        }
    }

    /// Create a new Claude client with custom configuration
    pub fn with_config(config: ClaudeConfig) -> Self {
        Self { config }
    }

    /// Query Claude CLI with retry logic
    pub async fn query(&self, prompt: &str) -> Result<String, Error> {
        let mut attempts = 0;
        let mut backoff_ms = 1000;

        loop {
            attempts += 1;
            debug!("Claude query attempt {} of {}", attempts, self.config.max_retries);

            match self.query_once(prompt).await {
                Ok(response) => return Ok(response),
                Err(e) if attempts >= self.config.max_retries => {
                    warn!("Claude query failed after {} attempts", attempts);
                    return Err(e);
                }
                Err(e) => {
                    if self.should_retry(&e) {
                        warn!("Claude query failed (attempt {}), retrying in {}ms: {}", attempts, backoff_ms, e);
                        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                        backoff_ms *= 2; // Exponential backoff
                    } else {
                        warn!("Claude query failed with non-retryable error: {}", e);
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Execute a single query attempt without retry
    async fn query_once(&self, prompt: &str) -> Result<String, Error> {
        // Build command: claude exec --json -s read-only "prompt"
        let mut cmd = Command::new("claude");
        cmd.args(["exec", "--json", "-s", "read-only", prompt])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        debug!("Executing: claude exec --json -s read-only [prompt: {} chars]", prompt.len());

        // Execute with timeout
        let timeout_duration = Duration::from_secs(self.config.timeout_secs);
        let child = cmd.spawn().map_err(|e| {
            Error::Llm(LlmError::RequestFailed {
                model: "claude".to_string(),
                source: format!("Failed to spawn process: {}", e),
            })
        })?;

        let output = tokio::time::timeout(timeout_duration, child.wait_with_output())
            .await
            .map_err(|_| {
                Error::Llm(LlmError::RequestFailed {
                    model: "claude".to_string(),
                    source: format!("Timeout after {}s", self.config.timeout_secs),
                })
            })??
            .map_err(|e| {
                Error::Llm(LlmError::RequestFailed {
                    model: "claude".to_string(),
                    source: format!("Process error: {}", e),
                })
            })?;

        // Check exit code
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(self.parse_error(&stderr));
        }

        // Parse JSON response
        let stdout = String::from_utf8(output.stdout).map_err(|e| {
            Error::Llm(LlmError::InvalidResponse {
                model: "claude".to_string(),
                details: format!("Invalid UTF-8 in output: {}", e),
            })
        })?;

        let response: ClaudeResponse = serde_json::from_str(&stdout).map_err(|e| {
            Error::Llm(LlmError::InvalidResponse {
                model: "claude".to_string(),
                details: format!("Failed to parse JSON: {}. Output: {}", e, stdout.chars().take(200).collect::<String>()),
            })
        })?;

        debug!("Claude query completed successfully");
        Ok(response.agent_message)
    }

    /// Parse error from stderr to determine error type
    fn parse_error(&self, stderr: &str) -> Error {
        let lower = stderr.to_lowercase();
        
        // Check for rate limit indicators
        if lower.contains("429") || lower.contains("rate limit") || lower.contains("quota exceeded") {
            // Try to extract retry-after from stderr
            let retry_after = self.extract_retry_after(stderr);
            return Error::Llm(LlmError::RateLimitExceeded {
                model: "claude".to_string(),
                retry_after,
            });
        }

        // Check for authentication errors
        if lower.contains("unauthorized") || lower.contains("authentication") || lower.contains("401") {
            return Error::Llm(LlmError::AuthenticationFailed("claude".to_string()));
        }

        // Check for model unavailable (503)
        if lower.contains("503") || lower.contains("unavailable") || lower.contains("service unavailable") {
            return Error::Llm(LlmError::ModelUnavailable("claude".to_string()));
        }

        // Generic error
        Error::Llm(LlmError::RequestFailed {
            model: "claude".to_string(),
            source: stderr.to_string(),
        })
    }

    /// Extract retry-after duration from error message
    fn extract_retry_after(&self, stderr: &str) -> Option<u64> {
        // Look for patterns like "retry after 60 seconds" or "retry-after: 60"
        let re = regex::Regex::new(r"(?i)retry[- ]after:?\s*(\d+)").ok()?;
        re.captures(stderr)?
            .get(1)?
            .as_str()
            .parse()
            .ok()
    }

    /// Check if error should be retried
    fn should_retry(&self, error: &Error) -> bool {
        matches!(
            error,
            Error::Llm(LlmError::RequestFailed { .. })
                | Error::Llm(LlmError::RateLimitExceeded { .. })
                | Error::Llm(LlmError::ModelUnavailable(_))
        )
    }
}

impl Default for ClaudeClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Response from Claude CLI (JSON format)
#[derive(Debug, Deserialize, Serialize)]
pub struct ClaudeResponse {
    /// The agent's response text
    pub agent_message: String,
    /// Status indicator (usually "success")
    #[serde(default)]
    pub status: String,
}

#[async_trait::async_trait]
impl crate::llm::LLMProvider for ClaudeClient {
    async fn query(&self, prompt: &str) -> Result<String, Error> {
        self.query(prompt).await
    }

    fn name(&self) -> &str {
        "claude"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = ClaudeConfig::default();
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_parse_rate_limit_error() {
        let client = ClaudeClient::new();
        let stderr = "Error: 429 Too Many Requests - rate limit exceeded";
        let error = client.parse_error(stderr);
        assert!(matches!(
            error,
            Error::Llm(LlmError::RateLimitExceeded { .. })
        ));
    }

    #[test]
    fn test_parse_auth_error() {
        let client = ClaudeClient::new();
        let stderr = "Error: 401 Unauthorized - authentication failed";
        let error = client.parse_error(stderr);
        assert!(matches!(
            error,
            Error::Llm(LlmError::AuthenticationFailed(_))
        ));
    }

    #[test]
    fn test_parse_unavailable_error() {
        let client = ClaudeClient::new();
        let stderr = "Error: 503 Service Unavailable";
        let error = client.parse_error(stderr);
        assert!(matches!(
            error,
            Error::Llm(LlmError::ModelUnavailable(_))
        ));
    }

    #[test]
    fn test_extract_retry_after() {
        let client = ClaudeClient::new();
        assert_eq!(
            client.extract_retry_after("retry after 60 seconds"),
            Some(60)
        );
        assert_eq!(
            client.extract_retry_after("retry-after: 120"),
            Some(120)
        );
        assert_eq!(client.extract_retry_after("no retry info"), None);
    }

    #[test]
    fn test_should_retry() {
        let client = ClaudeClient::new();
        let retryable = Error::Llm(LlmError::RateLimitExceeded {
            model: "claude".to_string(),
            retry_after: None,
        });
        assert!(client.should_retry(&retryable));

        let not_retryable = Error::Llm(LlmError::AuthenticationFailed("claude".to_string()));
        assert!(!client.should_retry(&not_retryable));
    }

    #[test]
    fn test_deserialize_claude_response() {
        let json = r#"{"agent_message": "Hello world", "status": "success"}"#;
        let response: ClaudeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.agent_message, "Hello world");
        assert_eq!(response.status, "success");
    }
}
