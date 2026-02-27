//! Query engine for searching ARF knowledge files.
//!
//! Searches `.noggin/` directory for ARF files matching a query string,
//! ranks results by match location and category, and returns structured
//! results with context.

use crate::arf::ArfFile;
use anyhow::{Context, Result};
use regex::RegexBuilder;
use serde::Serialize;
use std::path::PathBuf;
use walkdir::WalkDir;

/// Options controlling query behavior
#[derive(Debug, Clone)]
pub struct QueryOptions {
    /// Maximum number of results to return
    pub max_results: usize,
    /// Filter to a specific category (decisions, patterns, bugs, migrations, facts)
    pub category: Option<String>,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            max_results: 10,
            category: None,
        }
    }
}

/// A single query result with matched ARF and ranking info
#[derive(Debug, Clone, Serialize)]
pub struct QueryResult {
    /// Path to the ARF file relative to .noggin/
    pub file_path: String,
    /// Category inferred from directory (decisions, patterns, etc.)
    pub category: String,
    /// The ARF content
    pub what: String,
    pub why: String,
    pub how: String,
    /// Which field(s) matched the query
    pub matched_fields: Vec<String>,
    /// Relevance score (higher is better)
    pub score: f64,
}

/// Query engine that searches ARF files in .noggin/
pub struct QueryEngine {
    noggin_path: PathBuf,
}

impl QueryEngine {
    pub fn new(noggin_path: PathBuf) -> Self {
        Self { noggin_path }
    }

    /// Search ARF files for the given query string.
    ///
    /// Uses case-insensitive regex matching across what/why/how fields.
    /// Results are ranked by match location (what > why > how) and category
    /// weight (decisions > patterns > bugs > migrations > facts).
    pub fn search(&self, query: &str, opts: &QueryOptions) -> Result<Vec<QueryResult>> {
        let pattern = RegexBuilder::new(&regex::escape(query))
            .case_insensitive(true)
            .build()
            .context("Failed to build search regex")?;

        let mut results = Vec::new();

        for entry in WalkDir::new(&self.noggin_path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Only search .arf files
            if path.extension().map(|e| e != "arf").unwrap_or(true) {
                continue;
            }

            // Extract category from directory name
            let category = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            // Apply category filter
            if let Some(ref filter) = opts.category {
                if &category != filter {
                    continue;
                }
            }

            // Parse ARF file
            let arf = match ArfFile::from_toml(path) {
                Ok(a) => a,
                Err(_) => continue, // skip malformed files
            };

            // Check which fields match
            let mut matched_fields = Vec::new();
            let mut score = 0.0;

            if pattern.is_match(&arf.what) {
                matched_fields.push("what".to_string());
                score += 10.0;
            }
            if pattern.is_match(&arf.why) {
                matched_fields.push("why".to_string());
                score += 5.0;
            }
            if pattern.is_match(&arf.how) {
                matched_fields.push("how".to_string());
                score += 3.0;
            }

            if matched_fields.is_empty() {
                continue;
            }

            // Category weight bonus
            score += category_weight(&category);

            let rel_path = path
                .strip_prefix(&self.noggin_path)
                .unwrap_or(path)
                .display()
                .to_string();

            results.push(QueryResult {
                file_path: rel_path,
                category,
                what: arf.what,
                why: arf.why,
                how: arf.how,
                matched_fields,
                score,
            });
        }

        // Sort by score descending
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // Limit results
        results.truncate(opts.max_results);

        Ok(results)
    }
}

/// Category weight for ranking (higher = more important)
fn category_weight(category: &str) -> f64 {
    match category {
        "decisions" => 3.0,
        "patterns" => 2.5,
        "bugs" => 2.0,
        "migrations" => 1.5,
        "facts" => 1.0,
        _ => 0.5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arf::ArfFile;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn setup_test_noggin(dir: &Path) {
        let decisions = dir.join("decisions");
        let patterns = dir.join("patterns");
        let bugs = dir.join("bugs");
        fs::create_dir_all(&decisions).unwrap();
        fs::create_dir_all(&patterns).unwrap();
        fs::create_dir_all(&bugs).unwrap();

        ArfFile::new(
            "Use tokio for async runtime",
            "Need async I/O for LLM calls",
            "Add tokio dependency with full features",
        )
        .to_toml(&decisions.join("use-tokio.arf"))
        .unwrap();

        ArfFile::new(
            "Adopt serde for serialization",
            "Standard Rust serialization library",
            "Derive Serialize/Deserialize on all data types",
        )
        .to_toml(&decisions.join("adopt-serde.arf"))
        .unwrap();

        ArfFile::new(
            "Error handling with anyhow",
            "Ergonomic error propagation",
            "Use anyhow::Result everywhere, context() for wrapping",
        )
        .to_toml(&patterns.join("error-handling.arf"))
        .unwrap();

        ArfFile::new(
            "Fix memory leak in async task",
            "Tasks were not being dropped on cancellation",
            "Add tokio::select! with cancellation token",
        )
        .to_toml(&bugs.join("memory-leak.arf"))
        .unwrap();
    }

    #[test]
    fn test_basic_search() {
        let tmp = TempDir::new().unwrap();
        setup_test_noggin(tmp.path());

        let engine = QueryEngine::new(tmp.path().to_path_buf());
        let results = engine.search("tokio", &QueryOptions::default()).unwrap();

        assert!(results.len() >= 2); // tokio appears in decisions and bugs
        assert_eq!(results[0].category, "decisions"); // higher category weight
    }

    #[test]
    fn test_case_insensitive() {
        let tmp = TempDir::new().unwrap();
        setup_test_noggin(tmp.path());

        let engine = QueryEngine::new(tmp.path().to_path_buf());
        let results = engine.search("SERDE", &QueryOptions::default()).unwrap();

        assert!(!results.is_empty());
        assert!(results[0].what.contains("serde"));
    }

    #[test]
    fn test_category_filter() {
        let tmp = TempDir::new().unwrap();
        setup_test_noggin(tmp.path());

        let engine = QueryEngine::new(tmp.path().to_path_buf());
        let opts = QueryOptions {
            category: Some("bugs".to_string()),
            ..Default::default()
        };
        let results = engine.search("tokio", &opts).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].category, "bugs");
    }

    #[test]
    fn test_max_results() {
        let tmp = TempDir::new().unwrap();
        setup_test_noggin(tmp.path());

        let engine = QueryEngine::new(tmp.path().to_path_buf());
        let opts = QueryOptions {
            max_results: 1,
            ..Default::default()
        };
        let results = engine.search("tokio", &opts).unwrap();

        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_no_matches() {
        let tmp = TempDir::new().unwrap();
        setup_test_noggin(tmp.path());

        let engine = QueryEngine::new(tmp.path().to_path_buf());
        let results = engine
            .search("nonexistent_term_xyz", &QueryOptions::default())
            .unwrap();

        assert!(results.is_empty());
    }

    #[test]
    fn test_matched_fields_tracking() {
        let tmp = TempDir::new().unwrap();
        setup_test_noggin(tmp.path());

        let engine = QueryEngine::new(tmp.path().to_path_buf());
        let results = engine.search("anyhow", &QueryOptions::default()).unwrap();

        // "anyhow" appears in what, why (no), and how of the error-handling pattern
        let pattern_result = results
            .iter()
            .find(|r| r.category == "patterns")
            .expect("should find patterns result");

        assert!(pattern_result.matched_fields.contains(&"what".to_string()));
        assert!(pattern_result.matched_fields.contains(&"how".to_string()));
    }

    #[test]
    fn test_score_ranking() {
        let tmp = TempDir::new().unwrap();
        setup_test_noggin(tmp.path());

        let engine = QueryEngine::new(tmp.path().to_path_buf());
        let results = engine.search("error", &QueryOptions::default()).unwrap();

        // Results should be sorted by score descending
        for window in results.windows(2) {
            assert!(window[0].score >= window[1].score);
        }
    }

    #[test]
    fn test_empty_noggin_dir() {
        let tmp = TempDir::new().unwrap();
        // Don't create any subdirs or files

        let engine = QueryEngine::new(tmp.path().to_path_buf());
        let results = engine.search("anything", &QueryOptions::default()).unwrap();

        assert!(results.is_empty());
    }

    #[test]
    fn test_skips_non_arf_files() {
        let tmp = TempDir::new().unwrap();
        let decisions = tmp.path().join("decisions");
        fs::create_dir_all(&decisions).unwrap();

        // Write an ARF file and a non-ARF file
        ArfFile::new("Test decision", "Reason", "Steps")
            .to_toml(&decisions.join("test.arf"))
            .unwrap();
        fs::write(decisions.join("notes.txt"), "tokio is great").unwrap();

        let engine = QueryEngine::new(tmp.path().to_path_buf());
        let results = engine.search("tokio", &QueryOptions::default()).unwrap();

        // Should not match the .txt file
        assert!(results.is_empty());
    }

    #[test]
    fn test_json_serialization() {
        let result = QueryResult {
            file_path: "decisions/use-tokio.arf".to_string(),
            category: "decisions".to_string(),
            what: "Use tokio".to_string(),
            why: "Async".to_string(),
            how: "Add dep".to_string(),
            matched_fields: vec!["what".to_string()],
            score: 13.0,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"category\":\"decisions\""));
        assert!(json.contains("\"score\":13.0"));
    }
}
