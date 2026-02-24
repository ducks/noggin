//! LLM provider abstraction and implementations
//!
//! Supports multiple LLM providers (Claude, Codex, Gemini) via subprocess invocation.
//! Each provider implements the LLMProvider trait for consistent querying.

pub mod claude;
pub mod codex;
pub mod gemini;

use crate::error::{Error, LlmError};

/// Common trait for LLM providers
#[async_trait::async_trait]
pub trait LLMProvider: Send + Sync {
    /// Query the LLM with a prompt and return the response
    async fn query(&self, prompt: &str) -> Result<String, Error>;
    
    /// Get the provider name (e.g., "claude", "codex")
    fn name(&self) -> &str;
}
