//! Parallel multi-model analysis
//!
//! Spawns Claude, Codex, and Gemini concurrently via tokio,
//! collects outputs, and handles partial failures gracefully.
//! If at least one model succeeds, the analysis proceeds.

use crate::error::{Error, LlmError};
use crate::llm::LLMProvider;
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Result from a single model's analysis
#[derive(Debug, Clone)]
pub struct ModelResult {
    /// Provider name (e.g., "claude", "codex", "gemini")
    pub model: String,
    /// The model's response text
    pub response: String,
}

/// Result from parallel analysis across all models
#[derive(Debug)]
pub struct ParallelResult {
    /// Successful model responses
    pub successes: Vec<ModelResult>,
    /// Failed model names with their errors
    pub failures: Vec<ModelFailure>,
}

/// A single model failure
#[derive(Debug)]
pub struct ModelFailure {
    /// Provider name
    pub model: String,
    /// Error description
    pub error: String,
}

impl ParallelResult {
    /// Check if at least one model succeeded
    pub fn has_results(&self) -> bool {
        !self.successes.is_empty()
    }

    /// Get the number of successful responses
    pub fn success_count(&self) -> usize {
        self.successes.len()
    }

    /// Get the number of failures
    pub fn failure_count(&self) -> usize {
        self.failures.len()
    }

    /// Get responses as a map of model name -> response text
    pub fn responses(&self) -> HashMap<String, String> {
        self.successes
            .iter()
            .map(|r| (r.model.clone(), r.response.clone()))
            .collect()
    }
}

/// Run a prompt against multiple LLM providers in parallel.
///
/// All providers are spawned concurrently. Partial failures are tolerated
/// as long as at least one provider returns a result. If all providers
/// fail, returns an error.
pub async fn query_all(
    providers: &[Box<dyn LLMProvider>],
    prompt: &str,
) -> Result<ParallelResult, Error> {
    if providers.is_empty() {
        return Err(Error::Llm(LlmError::RequestFailed {
            model: "parallel".to_string(),
            source: "No providers configured".to_string(),
        }));
    }

    info!("Starting parallel analysis with {} providers", providers.len());

    // Build futures for all providers, then await them concurrently
    let futures: Vec<_> = providers
        .iter()
        .map(|provider| {
            let name = provider.name().to_string();
            debug!("Spawning query for {}", name);
            async move {
                let result = provider.query(prompt).await;
                (name, result)
            }
        })
        .collect();

    let results = futures::future::join_all(futures).await;

    let mut successes = Vec::new();
    let mut failures = Vec::new();

    for (name, result) in results {
        match result {
            Ok(response) => {
                info!("{} query succeeded ({} chars)", name, response.len());
                successes.push(ModelResult {
                    model: name,
                    response,
                });
            }
            Err(e) => {
                warn!("{} query failed: {}", name, e);
                failures.push(ModelFailure {
                    model: name,
                    error: e.to_string(),
                });
            }
        }
    }

    let result = ParallelResult {
        successes,
        failures,
    };

    if !result.has_results() {
        let models: Vec<_> = result.failures.iter().map(|f| f.model.as_str()).collect();
        return Err(Error::Llm(LlmError::RequestFailed {
            model: "parallel".to_string(),
            source: format!(
                "All {} providers failed: {}",
                result.failure_count(),
                models.join(", ")
            ),
        }));
    }

    info!(
        "Parallel analysis complete: {}/{} succeeded",
        result.success_count(),
        result.success_count() + result.failure_count()
    );

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    /// Mock provider that succeeds with a fixed response
    struct MockProvider {
        name: String,
        response: String,
    }

    #[async_trait]
    impl LLMProvider for MockProvider {
        async fn query(&self, _prompt: &str) -> Result<String, Error> {
            Ok(self.response.clone())
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    /// Mock provider that always fails
    struct FailingProvider {
        name: String,
    }

    #[async_trait]
    impl LLMProvider for FailingProvider {
        async fn query(&self, _prompt: &str) -> Result<String, Error> {
            Err(Error::Llm(LlmError::RequestFailed {
                model: self.name.clone(),
                source: "mock failure".to_string(),
            }))
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    #[tokio::test]
    async fn test_all_succeed() {
        let providers: Vec<Box<dyn LLMProvider>> = vec![
            Box::new(MockProvider {
                name: "claude".to_string(),
                response: "claude says hello".to_string(),
            }),
            Box::new(MockProvider {
                name: "codex".to_string(),
                response: "codex says hello".to_string(),
            }),
            Box::new(MockProvider {
                name: "gemini".to_string(),
                response: "gemini says hello".to_string(),
            }),
        ];

        let result = query_all(&providers, "test prompt").await.unwrap();
        assert_eq!(result.success_count(), 3);
        assert_eq!(result.failure_count(), 0);
        assert!(result.has_results());

        let responses = result.responses();
        assert_eq!(responses["claude"], "claude says hello");
        assert_eq!(responses["codex"], "codex says hello");
        assert_eq!(responses["gemini"], "gemini says hello");
    }

    #[tokio::test]
    async fn test_partial_failure() {
        let providers: Vec<Box<dyn LLMProvider>> = vec![
            Box::new(MockProvider {
                name: "claude".to_string(),
                response: "claude response".to_string(),
            }),
            Box::new(FailingProvider {
                name: "codex".to_string(),
            }),
            Box::new(MockProvider {
                name: "gemini".to_string(),
                response: "gemini response".to_string(),
            }),
        ];

        let result = query_all(&providers, "test prompt").await.unwrap();
        assert_eq!(result.success_count(), 2);
        assert_eq!(result.failure_count(), 1);
        assert!(result.has_results());
        assert_eq!(result.failures[0].model, "codex");
    }

    #[tokio::test]
    async fn test_all_fail() {
        let providers: Vec<Box<dyn LLMProvider>> = vec![
            Box::new(FailingProvider {
                name: "claude".to_string(),
            }),
            Box::new(FailingProvider {
                name: "codex".to_string(),
            }),
        ];

        let result = query_all(&providers, "test prompt").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("All 2 providers failed"));
    }

    #[tokio::test]
    async fn test_no_providers() {
        let providers: Vec<Box<dyn LLMProvider>> = vec![];
        let result = query_all(&providers, "test prompt").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("No providers configured"));
    }

    #[tokio::test]
    async fn test_single_provider() {
        let providers: Vec<Box<dyn LLMProvider>> = vec![
            Box::new(MockProvider {
                name: "claude".to_string(),
                response: "solo response".to_string(),
            }),
        ];

        let result = query_all(&providers, "test prompt").await.unwrap();
        assert_eq!(result.success_count(), 1);
        assert_eq!(result.failure_count(), 0);
    }

    #[test]
    fn test_parallel_result_responses_map() {
        let result = ParallelResult {
            successes: vec![
                ModelResult {
                    model: "a".to_string(),
                    response: "response_a".to_string(),
                },
                ModelResult {
                    model: "b".to_string(),
                    response: "response_b".to_string(),
                },
            ],
            failures: vec![],
        };

        let map = result.responses();
        assert_eq!(map.len(), 2);
        assert_eq!(map["a"], "response_a");
        assert_eq!(map["b"], "response_b");
    }
}
