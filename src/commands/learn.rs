//! Learn command: orchestrates the full codebase analysis pipeline.
//!
//! Scans files, walks git history, invokes LLMs in parallel,
//! synthesizes consensus, writes ARF files, and updates the manifest.

use crate::git::scoring::{score_commit, ScoreCategory, ScoringConfig};
use crate::git::walker::{walk_commits, WalkOptions};
use crate::learn::prompts::{build_commit_analysis_prompt, build_file_analysis_prompt};
use crate::learn::scanner::scan_files;
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
use std::env;
use tracing::info;

/// Run the learn command.
///
/// If `full` is true, ignores the manifest and re-analyzes everything.
/// If `verify` is true, shows what would be done without writing anything.
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
        "Scanned {} files ({} changed, {} unchanged)",
        scan_result.total,
        scan_result.changed.len(),
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

    // Step 4: Check if there's work to do
    if scan_result.changed.is_empty() && significant_commits.is_empty() {
        println!("Nothing to learn. Codebase is up to date.");
        return Ok(());
    }

    // Step 5: Build prompts
    let mut prompts = Vec::new();

    if !scan_result.changed.is_empty() {
        let file_prompt = build_file_analysis_prompt(&repo_path, &scan_result.changed);
        prompts.push(("files".to_string(), file_prompt));
    }

    if !significant_commits.is_empty() {
        let commit_prompt = build_commit_analysis_prompt(&significant_commits);
        prompts.push(("commits".to_string(), commit_prompt));
    }

    // Step 6: Invoke LLMs in parallel
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

    // Step 7: Synthesize consensus
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

    // Step 8: Verify mode - just print what would happen
    if verify {
        println!("\n--- Verify Mode (no files written) ---");
        println!("Would write {} ARF files:", unified_arfs.len());
        for arf in &unified_arfs {
            println!("  - {}", arf.what);
        }
        println!("{} file hashes would be updated", scan_result.changed.len());
        println!(
            "{} commits would be marked as processed",
            significant_commits.len()
        );
        print_warnings(&warnings);
        return Ok(());
    }

    // Step 9: Write ARF files
    if !unified_arfs.is_empty() {
        let pb = spinner("Writing ARF files...");
        let write_result = write_arfs(&noggin_path, &unified_arfs)
            .context("Failed to write ARF files")?;
        pb.finish_with_message(format!(
            "Wrote {} new, {} updated, {} skipped ARF files",
            write_result.written, write_result.updated, write_result.skipped
        ));
    }

    // Step 10: Update manifest
    let pb = spinner("Updating manifest...");

    // Update file hashes
    for file in &scan_result.changed {
        manifest.add_or_update_file(file.path.clone(), file.hash.clone(), vec![]);
    }

    // Update commit entries
    for commit in &significant_commits {
        let category = infer_commit_category(&commit.message_summary);
        manifest.add_commit(
            commit.hash.clone(),
            category,
            String::new(), // ARF path filled in later when we have better tracking
        );
    }

    manifest
        .save(&manifest_path)
        .context("Failed to save manifest")?;

    pb.finish_with_message("Manifest updated");

    // Step 11: Print summary
    println!();
    println!("=== Learn Complete ===");
    println!("  Files analyzed:    {}", scan_result.changed.len());
    println!("  Commits processed: {}", significant_commits.len());
    println!("  ARF entries:       {}", unified_arfs.len());

    print_warnings(&warnings);

    Ok(())
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
}
