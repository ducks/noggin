//! Learn command: orchestrates the full codebase analysis pipeline.
//!
//! Scans files, walks git history, invokes LLMs in parallel,
//! synthesizes consensus, writes ARF files, and updates the manifest.
//!
//! In incremental mode (default), only changed files and new commits are
//! processed. Patterns referencing changed files are invalidated and
//! re-analyzed. Deleted files are cleaned from the manifest.

use crate::git::scoring::{score_commit, ScoreCategory, ScoringConfig};
use crate::git::walker::{walk_commits, WalkOptions};
use crate::learn::prompts::{
    build_commit_analysis_prompt, build_file_analysis_prompt,
    build_pattern_reanalysis_prompt,
};
use crate::learn::scanner::{scan_files, FileToAnalyze};
use crate::learn::writer::write_arfs;
use crate::llm::claude::ClaudeClient;
use crate::llm::codex::CodexClient;
use crate::llm::gemini::GeminiClient;
use crate::llm::parallel::query_all;
use crate::llm::LLMProvider;
use crate::manifest::{CommitCategory, Manifest};
use crate::synthesis::{self, ModelOutput};
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::env;
use std::path::Path;
use tracing::info;

/// Run the learn command.
///
/// If `full` is true, ignores the manifest and re-analyzes everything.
/// If `verify` is true, shows what would be done without writing anything.
/// Returns Ok(()) on success. In verify mode, returns an error if drift
/// is detected (for use as a CI check).
pub async fn learn_command(full: bool, verify: bool) -> Result<()> {
    let repo_path = env::current_dir()?;
    let noggin_path = repo_path.join(".noggin");

    // Check .noggin/ exists
    if !noggin_path.exists() {
        anyhow::bail!(
            ".noggin/ directory not found. Run 'noggin init' first."
        );
    }

    let manifest_path = noggin_path.join("manifest.toml");

    // Step 1: Load manifest
    let mut manifest = Manifest::load(&manifest_path)
        .context("Failed to load manifest")?;

    let mode = if full { "full" } else { "incremental" };
    println!("Starting {} analysis...", mode);

    // Step 2: Scan files
    let pb = spinner("Scanning files...");
    let scan_result = scan_files(&repo_path, &manifest, full)
        .context("Failed to scan files")?;
    pb.finish_with_message(format!(
        "Scanned {} files ({} changed, {} deleted, {} unchanged)",
        scan_result.total,
        scan_result.changed.len(),
        scan_result.deleted.len(),
        scan_result.unchanged
    ));

    // Step 3: Walk git history
    let pb = spinner("Walking git history...");
    let walk_result = walk_commits(
        &repo_path,
        WalkOptions {
            skip_merges: true,
            ..Default::default()
        },
    )
    .context("Failed to walk git history")?;

    // Filter to unprocessed commits
    let unprocessed: Vec<_> = if full {
        walk_result.commits
    } else {
        walk_result
            .commits
            .into_iter()
            .filter(|c| !manifest.is_commit_processed(&c.hash))
            .collect()
    };

    // Score and filter to Medium+ significance
    let repo = git2::Repository::open(&repo_path)?;
    let scoring_config = ScoringConfig::default();
    let significant_commits: Vec<_> = unprocessed
        .into_iter()
        .filter(|cm| {
            if let Ok(commit) = repo.find_commit(git2::Oid::from_str(&cm.hash).unwrap()) {
                if let Ok(score) = score_commit(&repo, &commit, &scoring_config) {
                    return matches!(
                        score.category,
                        ScoreCategory::Critical | ScoreCategory::High | ScoreCategory::Medium
                    );
                }
            }
            false
        })
        .collect();

    pb.finish_with_message(format!(
        "Found {} significant commits",
        significant_commits.len()
    ));

    // Step 4: Detect invalidated patterns from changed/deleted files
    let invalidated_patterns = find_invalidated_patterns(
        &manifest,
        &scan_result.changed,
        &scan_result.deleted,
    );

    if !invalidated_patterns.is_empty() {
        println!(
            "  {} patterns invalidated by file changes",
            invalidated_patterns.len()
        );
    }

    // Step 5: Check if there's work to do
    let has_work = !scan_result.changed.is_empty()
        || !significant_commits.is_empty()
        || !scan_result.deleted.is_empty()
        || !invalidated_patterns.is_empty();

    if !has_work {
        println!("Nothing to learn. Codebase is up to date.");
        return Ok(());
    }

    // Step 6: Verify mode - report drift without updating
    if verify {
        println!("\n--- Verify Mode (no files written) ---");

        if !scan_result.changed.is_empty() {
            println!("{} files changed:", scan_result.changed.len());
            for f in &scan_result.changed {
                let label = if f.is_new { "new" } else { "modified" };
                println!("  {} [{}]", f.path, label);
            }
        }

        if !scan_result.deleted.is_empty() {
            println!("{} files deleted:", scan_result.deleted.len());
            for path in &scan_result.deleted {
                println!("  {}", path);
            }
        }

        if !significant_commits.is_empty() {
            println!("{} commits unprocessed:", significant_commits.len());
            for c in &significant_commits {
                println!("  {} {}", c.short_hash, c.message_summary);
            }
        }

        if !invalidated_patterns.is_empty() {
            println!("{} patterns need re-analysis:", invalidated_patterns.len());
            for p in &invalidated_patterns {
                println!("  {}", p);
            }
        }

        anyhow::bail!("Drift detected. Run 'noggin learn' to update.");
    }

    // Step 7: Build prompts
    let mut prompts = Vec::new();

    if !scan_result.changed.is_empty() {
        let file_prompt = build_file_analysis_prompt(&repo_path, &scan_result.changed);
        prompts.push(("files".to_string(), file_prompt));
    }

    if !significant_commits.is_empty() {
        let commit_prompt = build_commit_analysis_prompt(&significant_commits);
        prompts.push(("commits".to_string(), commit_prompt));
    }

    // Build re-analysis prompt for invalidated patterns
    if !invalidated_patterns.is_empty() {
        let pattern_files = collect_pattern_files(&manifest, &invalidated_patterns, &repo_path);
        if !pattern_files.is_empty() {
            let pattern_prompt = build_pattern_reanalysis_prompt(
                &repo_path,
                &invalidated_patterns,
                &pattern_files,
            );
            prompts.push(("patterns".to_string(), pattern_prompt));
        }
    }

    // Step 8: Invoke LLMs in parallel
    let providers: Vec<Box<dyn LLMProvider>> = vec![
        Box::new(ClaudeClient::new()),
        Box::new(CodexClient::new()),
        Box::new(GeminiClient::new()),
    ];

    let mut all_model_outputs: Vec<ModelOutput> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    for (prompt_type, prompt) in &prompts {
        let pb = spinner(&format!("Querying LLMs ({})...", prompt_type));

        match query_all(&providers, prompt).await {
            Ok(parallel_result) => {
                pb.finish_with_message(format!(
                    "LLM {} analysis: {}/{} models responded",
                    prompt_type,
                    parallel_result.success_count(),
                    parallel_result.success_count() + parallel_result.failure_count()
                ));

                for failure in &parallel_result.failures {
                    warnings.push(format!(
                        "{} failed for {} analysis: {}",
                        failure.model, prompt_type, failure.error
                    ));
                }

                // Parse responses into ModelOutput
                for model_result in &parallel_result.successes {
                    match synthesis::parse_model_response(
                        &model_result.model,
                        &model_result.response,
                    ) {
                        Ok(arfs) => {
                            info!(
                                "Parsed {} ARF entries from {} ({})",
                                arfs.len(),
                                model_result.model,
                                prompt_type
                            );
                            all_model_outputs.push(ModelOutput {
                                model_name: model_result.model.clone(),
                                arf_files: arfs,
                            });
                        }
                        Err(e) => {
                            warnings.push(format!(
                                "Failed to parse {} output for {}: {}",
                                model_result.model, prompt_type, e
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                pb.finish_with_message(format!("LLM {} analysis failed", prompt_type));
                warnings.push(format!("All LLMs failed for {} analysis: {}", prompt_type, e));
            }
        }
    }

    // Step 9: Synthesize consensus
    let unified_arfs = if all_model_outputs.is_empty() {
        warnings.push("No model outputs to synthesize".to_string());
        Vec::new()
    } else if all_model_outputs.len() == 1 {
        // Single model, skip synthesis
        info!("Single model output, skipping synthesis");
        all_model_outputs.remove(0).arf_files
    } else {
        let pb = spinner("Synthesizing consensus...");
        match synthesis::synthesize(all_model_outputs) {
            Ok(result) => {
                pb.finish_with_message(format!(
                    "Synthesized {} ARF entries ({} conflicts resolved)",
                    result.report.total_output_arfs, result.report.conflicts_resolved
                ));
                result.unified_arfs
            }
            Err(e) => {
                pb.finish_with_message("Synthesis failed");
                warnings.push(format!("Synthesis failed: {}", e));
                Vec::new()
            }
        }
    };

    // Step 10: Write ARF files
    if !unified_arfs.is_empty() {
        let pb = spinner("Writing ARF files...");
        let write_result = write_arfs(&noggin_path, &unified_arfs)
            .context("Failed to write ARF files")?;
        pb.finish_with_message(format!(
            "Wrote {} new, {} updated, {} skipped ARF files",
            write_result.written, write_result.updated, write_result.skipped
        ));
    }

    // Step 11: Update manifest
    let pb = spinner("Updating manifest...");

    // Remove deleted files
    for path in &scan_result.deleted {
        manifest.remove_file(path);
    }

    // Update file hashes
    for file in &scan_result.changed {
        manifest.add_or_update_file(file.path.clone(), file.hash.clone(), vec![]);
    }

    // Invalidate affected patterns
    for pattern_id in &invalidated_patterns {
        manifest.invalidate_pattern(pattern_id);
    }

    // Update commit entries
    for commit in &significant_commits {
        let category = infer_commit_category(&commit.message_summary);
        manifest.add_commit(
            commit.hash.clone(),
            category,
            String::new(),
        );
    }

    manifest
        .save(&manifest_path)
        .context("Failed to save manifest")?;

    pb.finish_with_message("Manifest updated");

    // Step 12: Print summary
    println!();
    println!("=== Learn Complete ===");
    println!("  Files analyzed:        {}", scan_result.changed.len());
    println!("  Files deleted:         {}", scan_result.deleted.len());
    println!("  Commits processed:     {}", significant_commits.len());
    println!("  Patterns invalidated:  {}", invalidated_patterns.len());
    println!("  ARF entries:           {}", unified_arfs.len());

    print_warnings(&warnings);

    Ok(())
}

/// Find patterns that need re-analysis due to changed or deleted files.
///
/// Looks up each changed/deleted file in the manifest to find patterns
/// that reference it. Returns the set of unique pattern IDs to re-analyze.
fn find_invalidated_patterns(
    manifest: &Manifest,
    changed: &[FileToAnalyze],
    deleted: &[String],
) -> Vec<String> {
    let mut invalidated: HashSet<String> = HashSet::new();

    for file in changed {
        for pattern_id in manifest.get_patterns_for_file(&file.path) {
            invalidated.insert(pattern_id);
        }
    }

    for path in deleted {
        for pattern_id in manifest.get_patterns_for_file(path) {
            invalidated.insert(pattern_id);
        }
    }

    let mut result: Vec<String> = invalidated.into_iter().collect();
    result.sort();
    result
}

/// Collect all contributing files for a set of patterns.
///
/// Returns FileToAnalyze structs for files that contribute to the
/// invalidated patterns (reading current content from disk).
fn collect_pattern_files(
    manifest: &Manifest,
    pattern_ids: &[String],
    repo_path: &Path,
) -> Vec<FileToAnalyze> {
    let mut files: HashSet<String> = HashSet::new();

    for pattern_id in pattern_ids {
        if let Some(pattern) = manifest.patterns.get(pattern_id) {
            for file_path in &pattern.contributing_files {
                files.insert(file_path.clone());
            }
        }
    }

    files
        .into_iter()
        .filter_map(|path| {
            let full_path = repo_path.join(&path);
            if !full_path.exists() {
                return None;
            }
            let metadata = std::fs::metadata(&full_path).ok()?;
            let hash = crate::manifest::calculate_file_hash(&full_path).ok()?;
            Some(FileToAnalyze {
                path,
                hash,
                size: metadata.len(),
                is_new: false,
                is_changed: true,
            })
        })
        .collect()
}

/// Infer a commit category from its message
fn infer_commit_category(message: &str) -> CommitCategory {
    let lower = message.to_lowercase();
    if lower.contains("migrat") || lower.contains("schema") || lower.contains("upgrade") {
        CommitCategory::Migration
    } else if lower.contains("fix") || lower.contains("bug") || lower.contains("patch") {
        CommitCategory::Bug
    } else {
        CommitCategory::Decision
    }
}

/// Create a spinner-style progress bar
fn spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}

/// Print collected warnings
fn print_warnings(warnings: &[String]) {
    if !warnings.is_empty() {
        println!();
        println!("Warnings:");
        for w in warnings {
            println!("  - {}", w);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn::scanner::FileToAnalyze;

    #[test]
    fn test_infer_commit_category_bug() {
        assert!(matches!(
            infer_commit_category("Fix memory leak in connection pool"),
            CommitCategory::Bug
        ));
        assert!(matches!(
            infer_commit_category("bug: patch null pointer"),
            CommitCategory::Bug
        ));
    }

    #[test]
    fn test_infer_commit_category_migration() {
        assert!(matches!(
            infer_commit_category("Add database migration for users table"),
            CommitCategory::Migration
        ));
        assert!(matches!(
            infer_commit_category("Schema upgrade to v3"),
            CommitCategory::Migration
        ));
    }

    #[test]
    fn test_infer_commit_category_decision() {
        assert!(matches!(
            infer_commit_category("Adopt tokio for async runtime"),
            CommitCategory::Decision
        ));
        assert!(matches!(
            infer_commit_category("Refactor authentication module"),
            CommitCategory::Decision
        ));
    }

    #[test]
    fn test_find_invalidated_patterns_from_changed_files() {
        let mut manifest = Manifest::default();
        manifest.add_or_update_file(
            "src/errors.rs".to_string(),
            "hash1".to_string(),
            vec!["error-handling".to_string()],
        );
        manifest.add_or_update_file(
            "src/api.rs".to_string(),
            "hash2".to_string(),
            vec!["api-patterns".to_string(), "error-handling".to_string()],
        );

        let changed = vec![FileToAnalyze {
            path: "src/errors.rs".to_string(),
            hash: "new_hash".to_string(),
            size: 100,
            is_new: false,
            is_changed: true,
        }];

        let result = find_invalidated_patterns(&manifest, &changed, &[]);

        assert_eq!(result, vec!["error-handling"]);
    }

    #[test]
    fn test_find_invalidated_patterns_from_deleted_files() {
        let mut manifest = Manifest::default();
        manifest.add_or_update_file(
            "src/old.rs".to_string(),
            "hash1".to_string(),
            vec!["legacy-patterns".to_string()],
        );

        let deleted = vec!["src/old.rs".to_string()];
        let result = find_invalidated_patterns(&manifest, &[], &deleted);

        assert_eq!(result, vec!["legacy-patterns"]);
    }

    #[test]
    fn test_find_invalidated_patterns_deduplicates() {
        let mut manifest = Manifest::default();
        manifest.add_or_update_file(
            "src/a.rs".to_string(),
            "hash1".to_string(),
            vec!["shared-pattern".to_string()],
        );
        manifest.add_or_update_file(
            "src/b.rs".to_string(),
            "hash2".to_string(),
            vec!["shared-pattern".to_string()],
        );

        let changed = vec![
            FileToAnalyze {
                path: "src/a.rs".to_string(),
                hash: "new1".to_string(),
                size: 100,
                is_new: false,
                is_changed: true,
            },
            FileToAnalyze {
                path: "src/b.rs".to_string(),
                hash: "new2".to_string(),
                size: 200,
                is_new: false,
                is_changed: true,
            },
        ];

        let result = find_invalidated_patterns(&manifest, &changed, &[]);

        // Should only appear once despite both files referencing it
        assert_eq!(result, vec!["shared-pattern"]);
    }

    #[test]
    fn test_find_invalidated_patterns_empty_when_no_patterns() {
        let mut manifest = Manifest::default();
        manifest.add_or_update_file(
            "src/main.rs".to_string(),
            "hash1".to_string(),
            vec![], // No patterns linked
        );

        let changed = vec![FileToAnalyze {
            path: "src/main.rs".to_string(),
            hash: "new_hash".to_string(),
            size: 100,
            is_new: false,
            is_changed: true,
        }];

        let result = find_invalidated_patterns(&manifest, &changed, &[]);

        assert!(result.is_empty());
    }
}
