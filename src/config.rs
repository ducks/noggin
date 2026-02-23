use crate::git::scoring::ScoringConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub scoring: ScoringConfig,
    #[serde(default)]
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default)]
    pub claude: ClaudeConfig,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            claude: ClaudeConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeConfig {
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_timeout() -> u64 {
    30
}

fn default_max_retries() -> u32 {
    3
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout(),
            max_retries: default_max_retries(),
        }
    }
}