//! Commit significance scoring based on diff size, file patterns, and message keywords.

use git2::{Commit, Diff, Repository};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Categories of commit significance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScoreCategory {
    Critical,
    High,
    Medium,
    Low,
    Trivial,
}

impl ScoreCategory {
    /// Convert a raw score (0.0-1.0) to a category
    pub fn from_score(score: f32) -> Self {
        match score {
            s if s >= 0.8 => ScoreCategory::Critical,
            s if s >= 0.6 => ScoreCategory::High,
            s if s >= 0.4 => ScoreCategory::Medium,
            s if s >= 0.2 => ScoreCategory::Low,
            _ => ScoreCategory::Trivial,
        }
    }
}

impl std::fmt::Display for ScoreCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScoreCategory::Critical => write!(f, "Critical"),
            ScoreCategory::High => write!(f, "High"),
            ScoreCategory::Medium => write!(f, "Medium"),
            ScoreCategory::Low => write!(f, "Low"),
            ScoreCategory::Trivial => write!(f, "Trivial"),
        }
    }
}

/// Factors contributing to a commit's score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScoreFactor {
    DiffSize { lines: usize, score: f32 },
    FilePattern { pattern: String, score: f32 },
    MessageKeyword { keyword: String, score: f32 },
}

/// Commit significance score with breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitScore {
    pub significance: f32,
    pub category: ScoreCategory,
    pub factors: Vec<ScoreFactor>,
}

/// Configuration for commit scoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringConfig {
    pub diff_weight: f32,
    pub pattern_weight: f32,
    pub message_weight: f32,
    pub file_patterns: HashMap<String, f32>,
    pub message_keywords: HashMap<String, f32>,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        let mut file_patterns = HashMap::new();
        file_patterns.insert("migrations/".to_string(), 1.0);
        file_patterns.insert("schema/".to_string(), 1.0);
        file_patterns.insert("core/".to_string(), 1.0);
        file_patterns.insert("lib/fundamentals/".to_string(), 1.0);
        file_patterns.insert("security/".to_string(), 1.0);
        file_patterns.insert("src/".to_string(), 0.8);
        file_patterns.insert("app/models/".to_string(), 0.8);
        file_patterns.insert("app/controllers/".to_string(), 0.8);
        file_patterns.insert("config/".to_string(), 0.8);
        file_patterns.insert("tests/".to_string(), 0.5);
        file_patterns.insert("specs/".to_string(), 0.5);
        file_patterns.insert("test/".to_string(), 0.5);
        file_patterns.insert("spec/".to_string(), 0.5);
        file_patterns.insert("docs/architecture/".to_string(), 0.5);
        file_patterns.insert("docs/".to_string(), 0.3);
        file_patterns.insert("README".to_string(), 0.3);
        file_patterns.insert("examples/".to_string(), 0.3);
        file_patterns.insert(".gitignore".to_string(), 0.1);
        file_patterns.insert(".editorconfig".to_string(), 0.1);
        
        let mut message_keywords = HashMap::new();
        message_keywords.insert("breaking change".to_string(), 1.0);
        message_keywords.insert("security fix".to_string(), 1.0);
        message_keywords.insert("cve-".to_string(), 1.0);
        message_keywords.insert("vulnerability".to_string(), 1.0);
        message_keywords.insert("refactor".to_string(), 0.8);
        message_keywords.insert("architecture".to_string(), 0.8);
        message_keywords.insert("migration".to_string(), 0.8);
        message_keywords.insert("deprecate".to_string(), 0.8);
        message_keywords.insert("feature".to_string(), 0.6);
        message_keywords.insert("enhancement".to_string(), 0.6);
        message_keywords.insert("optimize".to_string(), 0.6);
        message_keywords.insert("performance".to_string(), 0.6);
        message_keywords.insert("fix".to_string(), 0.4);
        message_keywords.insert("bug".to_string(), 0.4);
        message_keywords.insert("update".to_string(), 0.4);
        message_keywords.insert("typo".to_string(), 0.2);
        message_keywords.insert("whitespace".to_string(), 0.2);
        message_keywords.insert("formatting".to_string(), 0.2);
        message_keywords.insert("docs".to_string(), 0.2);
        
        Self {
            diff_weight: 0.3,
            pattern_weight: 0.4,
            message_weight: 0.3,
            file_patterns,
            message_keywords,
        }
    }
}

/// Score a commit's significance
pub fn score_commit(
    repo: &Repository,
    commit: &Commit,
    config: &ScoringConfig,
) -> anyhow::Result<CommitScore> {
    let mut factors = Vec::new();
    
    let diff_score = score_diff_size(repo, commit, &mut factors)?;
    let pattern_score = score_file_patterns(repo, commit, config, &mut factors)?;
    let message_score = score_message(commit, config, &mut factors);
    
    let significance = (diff_score * config.diff_weight)
        + (pattern_score * config.pattern_weight)
        + (message_score * config.message_weight);
    
    let category = ScoreCategory::from_score(significance);
    
    Ok(CommitScore {
        significance,
        category,
        factors,
    })
}

fn score_diff_size(
    repo: &Repository,
    commit: &Commit,
    factors: &mut Vec<ScoreFactor>,
) -> anyhow::Result<f32> {
    let parent_count = commit.parent_count();
    
    if parent_count == 0 || parent_count > 1 {
        return Ok(0.5);
    }
    
    let parent = commit.parent(0)?;
    let parent_tree = parent.tree()?;
    let commit_tree = commit.tree()?;
    
    let diff = repo.diff_tree_to_tree(Some(&parent_tree), Some(&commit_tree), None)?;
    let stats = diff.stats()?;
    
    let total_lines = stats.insertions() + stats.deletions();
    
    let is_trivial_change = is_trivial_diff(&diff)?;
    let multiplier = if is_trivial_change { 0.5 } else { 1.0 };
    
    let base_score = match total_lines {
        0..=10 => 0.1,
        11..=50 => 0.3,
        51..=200 => 0.5,
        201..=500 => 0.7,
        _ => 1.0,
    };
    
    let score = base_score * multiplier;
    
    factors.push(ScoreFactor::DiffSize {
        lines: total_lines,
        score,
    });
    
    Ok(score)
}

fn is_trivial_diff(diff: &Diff) -> anyhow::Result<bool> {
    let stats = diff.stats()?;
    let total = stats.insertions() + stats.deletions();
    
    if total <= 1 {
        return Ok(true);
    }
    
    let mut trivial_files = 0;
    let mut total_files = 0;
    
    diff.foreach(
        &mut |delta, _| {
            total_files += 1;
            if let Some(path) = delta.new_file().path() {
                if let Some(ext) = path.extension() {
                    if ext == "md" || ext == "txt" || ext == "rst" {
                        trivial_files += 1;
                    }
                }
            }
            true
        },
        None,
        None,
        None,
    )?;
    
    Ok(total_files > 0 && (trivial_files as f32 / total_files as f32) > 0.8)
}

fn score_file_patterns(
    repo: &Repository,
    commit: &Commit,
    config: &ScoringConfig,
    factors: &mut Vec<ScoreFactor>,
) -> anyhow::Result<f32> {
    let parent_count = commit.parent_count();
    
    if parent_count == 0 || parent_count > 1 {
        return Ok(0.5);
    }
    
    let parent = commit.parent(0)?;
    let parent_tree = parent.tree()?;
    let commit_tree = commit.tree()?;
    
    let diff = repo.diff_tree_to_tree(Some(&parent_tree), Some(&commit_tree), None)?;
    
    let mut max_score = 0.0;
    let mut max_pattern = String::new();
    
    diff.foreach(
        &mut |delta, _| {
            if let Some(path) = delta.new_file().path() {
                let path_str = path.to_string_lossy();
                
                for (pattern, score) in &config.file_patterns {
                    if path_str.contains(pattern) && *score > max_score {
                        max_score = *score;
                        max_pattern = pattern.clone();
                    }
                }
            }
            true
        },
        None,
        None,
        None,
    )?;
    
    if max_score > 0.0 {
        factors.push(ScoreFactor::FilePattern {
            pattern: max_pattern,
            score: max_score,
        });
    }
    
    Ok(max_score)
}

fn score_message(
    commit: &Commit,
    config: &ScoringConfig,
    factors: &mut Vec<ScoreFactor>,
) -> f32 {
    let message = commit.message().unwrap_or("").to_lowercase();
    
    let mut max_score = 0.0;
    let mut max_keyword = String::new();
    
    for (keyword, score) in &config.message_keywords {
        let keyword_lower = keyword.to_lowercase();
        if message.contains(&keyword_lower) && *score > max_score {
            max_score = *score;
            max_keyword = keyword.clone();
        }
    }
    
    if max_score > 0.0 {
        factors.push(ScoreFactor::MessageKeyword {
            keyword: max_keyword,
            score: max_score,
        });
    }
    
    max_score
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_score_category_from_score() {
        assert_eq!(ScoreCategory::from_score(0.9), ScoreCategory::Critical);
        assert_eq!(ScoreCategory::from_score(0.7), ScoreCategory::High);
        assert_eq!(ScoreCategory::from_score(0.5), ScoreCategory::Medium);
        assert_eq!(ScoreCategory::from_score(0.3), ScoreCategory::Low);
        assert_eq!(ScoreCategory::from_score(0.1), ScoreCategory::Trivial);
    }
    
    #[test]
    fn test_default_config() {
        let config = ScoringConfig::default();
        
        assert_eq!(config.diff_weight, 0.3);
        assert_eq!(config.pattern_weight, 0.4);
        assert_eq!(config.message_weight, 0.3);
        
        assert_eq!(config.file_patterns.get("migrations/"), Some(&1.0));
        assert_eq!(config.message_keywords.get("breaking change"), Some(&1.0));
    }
}
