//! Status command: shows the state of the noggin knowledge base.
//!
//! Reports files scanned, pending changes, unprocessed commits,
//! ARF file counts by category, and overall freshness.

use crate::git::walker::{walk_commits, WalkOptions};
use crate::learn::scanner::scan_files;
use crate::manifest::Manifest;
use anyhow::{Context, Result};
use colored::Colorize;
use serde::Serialize;
use std::env;
use std::fs;
use std::path::Path;

/// Status information collected for display
#[derive(Debug, Serialize)]
struct StatusInfo {
    repo_path: String,
    initialized: bool,
    files: FileStatus,
    commits: CommitStatus,
    knowledge: KnowledgeStatus,
    up_to_date: bool,
}

#[derive(Debug, Serialize)]
struct FileStatus {
    total: usize,
    scanned: usize,
    modified: usize,
    new: usize,
    deleted: usize,
    unchanged: usize,
}

#[derive(Debug, Serialize)]
struct CommitStatus {
    total: usize,
    processed: usize,
    unprocessed: usize,
}

#[derive(Debug, Serialize)]
struct KnowledgeStatus {
    total_arfs: usize,
    decisions: usize,
    patterns: usize,
    bugs: usize,
    migrations: usize,
    facts: usize,
}

/// Run the status command.
///
/// If `verbose` is true, shows detailed file and commit listings.
/// If `json` is true, outputs machine-readable JSON.
pub fn status_command(verbose: bool, json: bool) -> Result<()> {
    let repo_path = env::current_dir()?;
    let noggin_path = repo_path.join(".noggin");

    if !noggin_path.exists() {
        if json {
            let info = StatusInfo {
                repo_path: repo_path.display().to_string(),
                initialized: false,
                files: FileStatus {
                    total: 0, scanned: 0, modified: 0, new: 0, deleted: 0, unchanged: 0,
                },
                commits: CommitStatus { total: 0, processed: 0, unprocessed: 0 },
                knowledge: KnowledgeStatus {
                    total_arfs: 0, decisions: 0, patterns: 0, bugs: 0, migrations: 0, facts: 0,
                },
                up_to_date: false,
            };
            println!("{}", serde_json::to_string_pretty(&info)?);
        } else {
            println!(
                "{} Not initialized. Run {} to get started.",
                "noggin:".bold(),
                "'noggin init'".cyan()
            );
        }
        return Ok(());
    }

    let manifest_path = noggin_path.join("manifest.toml");
    let manifest = Manifest::load(&manifest_path)
        .context("Failed to load manifest")?;

    // Scan files
    let scan_result = scan_files(&repo_path, &manifest, false)
        .context("Failed to scan files")?;

    let modified_count = scan_result.changed.iter().filter(|f| f.is_changed).count();
    let new_count = scan_result.changed.iter().filter(|f| f.is_new).count();

    // Walk commits
    let walk_result = walk_commits(
        &repo_path,
        WalkOptions {
            skip_merges: true,
            ..Default::default()
        },
    )
    .context("Failed to walk git history")?;

    let total_commits = walk_result.commits.len();
    let unprocessed_commits: Vec<_> = walk_result
        .commits
        .iter()
        .filter(|c| !manifest.is_commit_processed(&c.hash))
        .collect();

    // Count ARF files by category
    let knowledge = count_arf_files(&noggin_path);

    let up_to_date = scan_result.changed.is_empty()
        && scan_result.deleted.is_empty()
        && unprocessed_commits.is_empty();

    let info = StatusInfo {
        repo_path: repo_path.display().to_string(),
        initialized: true,
        files: FileStatus {
            total: scan_result.total,
            scanned: manifest.files.len(),
            modified: modified_count,
            new: new_count,
            deleted: scan_result.deleted.len(),
            unchanged: scan_result.unchanged,
        },
        commits: CommitStatus {
            total: total_commits,
            processed: manifest.commits.len(),
            unprocessed: unprocessed_commits.len(),
        },
        knowledge,
        up_to_date,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&info)?);
        return Ok(());
    }

    // Human-readable output
    println!("{}", "Noggin Status".bold());
    println!("{}", repo_path.display().to_string().dimmed());
    println!();

    // Files section
    println!("{}", "Files".bold());
    println!(
        "  {} scanned, {} total",
        info.files.scanned.to_string().cyan(),
        info.files.total
    );
    if info.files.modified > 0 {
        println!(
            "  {} modified",
            info.files.modified.to_string().yellow()
        );
    }
    if info.files.new > 0 {
        println!(
            "  {} new",
            info.files.new.to_string().yellow()
        );
    }
    if info.files.deleted > 0 {
        println!(
            "  {} deleted",
            info.files.deleted.to_string().red()
        );
    }

    // Verbose: list changed files
    if verbose && !scan_result.changed.is_empty() {
        for file in &scan_result.changed {
            let label = if file.is_new {
                "new".green()
            } else {
                "modified".yellow()
            };
            println!("    {} [{}]", file.path.dimmed(), label);
        }
        for path in &scan_result.deleted {
            println!("    {} [{}]", path.dimmed(), "deleted".red());
        }
    }

    println!();

    // Commits section
    println!("{}", "Commits".bold());
    println!(
        "  {} processed, {} total",
        info.commits.processed.to_string().cyan(),
        info.commits.total
    );
    if info.commits.unprocessed > 0 {
        println!(
            "  {} unprocessed",
            info.commits.unprocessed.to_string().yellow()
        );
    }

    // Verbose: list unprocessed commits
    if verbose && !unprocessed_commits.is_empty() {
        let display_count = unprocessed_commits.len().min(20);
        for commit in &unprocessed_commits[..display_count] {
            println!(
                "    {} {}",
                commit.short_hash.dimmed(),
                commit.message_summary
            );
        }
        if unprocessed_commits.len() > 20 {
            println!(
                "    {} more...",
                (unprocessed_commits.len() - 20).to_string().dimmed()
            );
        }
    }

    println!();

    // Knowledge section
    println!("{}", "Knowledge Base".bold());
    println!(
        "  {} ARF files",
        info.knowledge.total_arfs.to_string().cyan()
    );
    if info.knowledge.total_arfs > 0 {
        let categories = [
            ("decisions", info.knowledge.decisions),
            ("patterns", info.knowledge.patterns),
            ("bugs", info.knowledge.bugs),
            ("migrations", info.knowledge.migrations),
            ("facts", info.knowledge.facts),
        ];
        let non_empty: Vec<_> = categories
            .iter()
            .filter(|(_, count)| *count > 0)
            .map(|(name, count)| format!("{} {}", count, name))
            .collect();
        if !non_empty.is_empty() {
            println!("  {}", non_empty.join(", "));
        }
    }

    // Patterns in manifest
    if !manifest.patterns.is_empty() {
        println!(
            "  {} patterns tracked",
            manifest.patterns.len().to_string().cyan()
        );
    }

    println!();

    // Freshness
    if up_to_date {
        println!("{}", "Up to date".green().bold());
    } else {
        let pending: Vec<String> = [
            if modified_count > 0 {
                Some(format!("{} modified files", modified_count))
            } else {
                None
            },
            if new_count > 0 {
                Some(format!("{} new files", new_count))
            } else {
                None
            },
            if !scan_result.deleted.is_empty() {
                Some(format!("{} deleted files", scan_result.deleted.len()))
            } else {
                None
            },
            if !unprocessed_commits.is_empty() {
                Some(format!("{} unprocessed commits", unprocessed_commits.len()))
            } else {
                None
            },
        ]
        .into_iter()
        .flatten()
        .collect();

        println!(
            "{} {}",
            "Pending:".yellow().bold(),
            pending.join(", ")
        );
        println!(
            "Run {} to process changes.",
            "'noggin learn'".cyan()
        );
    }

    Ok(())
}

/// Count .arf files in each category subdirectory
fn count_arf_files(noggin_path: &Path) -> KnowledgeStatus {
    let categories = [
        ("decisions", 0),
        ("patterns", 0),
        ("bugs", 0),
        ("migrations", 0),
        ("facts", 0),
    ];

    let mut status = KnowledgeStatus {
        total_arfs: 0,
        decisions: 0,
        patterns: 0,
        bugs: 0,
        migrations: 0,
        facts: 0,
    };

    for (dir_name, _) in &categories {
        let dir_path = noggin_path.join(dir_name);
        if let Ok(entries) = fs::read_dir(&dir_path) {
            let count = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|ext| ext == "arf")
                        .unwrap_or(false)
                })
                .count();

            match *dir_name {
                "decisions" => status.decisions = count,
                "patterns" => status.patterns = count,
                "bugs" => status.bugs = count,
                "migrations" => status.migrations = count,
                "facts" => status.facts = count,
                _ => {}
            }
            status.total_arfs += count;
        }
    }

    status
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_noggin_dir(temp_dir: &TempDir) {
        let noggin = temp_dir.path().join(".noggin");
        fs::create_dir_all(noggin.join("decisions")).unwrap();
        fs::create_dir_all(noggin.join("patterns")).unwrap();
        fs::create_dir_all(noggin.join("bugs")).unwrap();
        fs::create_dir_all(noggin.join("migrations")).unwrap();
        fs::create_dir_all(noggin.join("facts")).unwrap();

        let manifest = "# Noggin manifest\n\n[files]\n\n[commits]\n";
        fs::write(noggin.join("manifest.toml"), manifest).unwrap();
    }

    #[test]
    fn test_count_arf_files_empty() {
        let temp_dir = TempDir::new().unwrap();
        setup_noggin_dir(&temp_dir);

        let result = count_arf_files(&temp_dir.path().join(".noggin"));

        assert_eq!(result.total_arfs, 0);
        assert_eq!(result.decisions, 0);
        assert_eq!(result.patterns, 0);
    }

    #[test]
    fn test_count_arf_files_with_entries() {
        let temp_dir = TempDir::new().unwrap();
        setup_noggin_dir(&temp_dir);

        let noggin = temp_dir.path().join(".noggin");

        // Write some ARF files
        fs::write(
            noggin.join("decisions/use-tokio.arf"),
            "what = \"Use tokio\"\nwhy = \"Async\"\nhow = \"Add dep\"\n",
        )
        .unwrap();
        fs::write(
            noggin.join("decisions/adopt-serde.arf"),
            "what = \"Use serde\"\nwhy = \"Serialization\"\nhow = \"Derive\"\n",
        )
        .unwrap();
        fs::write(
            noggin.join("patterns/error-handling.arf"),
            "what = \"Anyhow pattern\"\nwhy = \"Ergonomic\"\nhow = \"Use anyhow\"\n",
        )
        .unwrap();
        fs::write(
            noggin.join("bugs/memory-leak.arf"),
            "what = \"Fix leak\"\nwhy = \"OOM\"\nhow = \"Drop impl\"\n",
        )
        .unwrap();

        // Write a non-arf file that should be ignored
        fs::write(noggin.join("decisions/notes.txt"), "not an arf").unwrap();

        let result = count_arf_files(&noggin);

        assert_eq!(result.total_arfs, 4);
        assert_eq!(result.decisions, 2);
        assert_eq!(result.patterns, 1);
        assert_eq!(result.bugs, 1);
        assert_eq!(result.migrations, 0);
        assert_eq!(result.facts, 0);
    }

    #[test]
    fn test_count_arf_files_missing_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let noggin = temp_dir.path().join(".noggin");
        fs::create_dir_all(&noggin).unwrap();
        // Don't create subdirectories

        let result = count_arf_files(&noggin);

        assert_eq!(result.total_arfs, 0);
    }

    #[test]
    fn test_status_info_serializes_to_json() {
        let info = StatusInfo {
            repo_path: "/tmp/test".to_string(),
            initialized: true,
            files: FileStatus {
                total: 50,
                scanned: 45,
                modified: 3,
                new: 2,
                deleted: 1,
                unchanged: 42,
            },
            commits: CommitStatus {
                total: 100,
                processed: 95,
                unprocessed: 5,
            },
            knowledge: KnowledgeStatus {
                total_arfs: 10,
                decisions: 3,
                patterns: 4,
                bugs: 1,
                migrations: 1,
                facts: 1,
            },
            up_to_date: false,
        };

        let json = serde_json::to_string_pretty(&info).unwrap();
        assert!(json.contains("\"initialized\": true"));
        assert!(json.contains("\"modified\": 3"));
        assert!(json.contains("\"unprocessed\": 5"));
        assert!(json.contains("\"total_arfs\": 10"));
        assert!(json.contains("\"up_to_date\": false"));
    }
}
