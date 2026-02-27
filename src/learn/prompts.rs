//! LLM prompt templates for codebase analysis.
//!
//! Generates structured prompts that instruct models to output
//! findings in TOML ARF format for parsing by the synthesis pipeline.

use crate::git::walker::CommitMetadata;
use crate::learn::scanner::FileToAnalyze;
use std::fs;
use std::path::Path;

/// Maximum lines to include per file in prompts
const MAX_LINES_PER_FILE: usize = 200;

/// Maximum files to include in a single prompt
const MAX_FILES_PER_PROMPT: usize = 50;

/// Build a prompt for analyzing source files.
///
/// Includes file paths and truncated contents, asks the model to
/// identify patterns, conventions, architecture decisions, and facts.
pub fn build_file_analysis_prompt(repo_path: &Path, files: &[FileToAnalyze]) -> String {
    let mut prompt = String::from(
        "Analyze the following source files from a codebase. \
         Identify architectural patterns, coding conventions, error handling \
         approaches, testing strategies, and notable design decisions.\n\n\
         Output your findings as TOML entries using this exact format:\n\n\
         ```\n\
         [[entry]]\n\
         what = \"one-sentence description of the finding\"\n\
         why = \"reasoning and motivation behind this pattern or decision\"\n\
         how = \"how it's implemented, key files, and relevant details\"\n\n\
         [entry.context]\n\
         files = [\"path/to/file.rs\"]\n\
         dependencies = [\"crate-name\"]\n\
         ```\n\n\
         Include multiple [[entry]] blocks. Focus on findings that would help \
         a developer understand the codebase architecture and conventions.\n\n\
         --- FILES ---\n\n",
    );

    let limit = files.len().min(MAX_FILES_PER_PROMPT);

    for file in &files[..limit] {
        let full_path = repo_path.join(&file.path);
        prompt.push_str(&format!("=== {} ({} bytes) ===\n", file.path, file.size));

        if let Ok(contents) = fs::read_to_string(&full_path) {
            let truncated: String = contents
                .lines()
                .take(MAX_LINES_PER_FILE)
                .collect::<Vec<_>>()
                .join("\n");
            prompt.push_str(&truncated);

            let line_count = contents.lines().count();
            if line_count > MAX_LINES_PER_FILE {
                prompt.push_str(&format!(
                    "\n... ({} more lines truncated)\n",
                    line_count - MAX_LINES_PER_FILE
                ));
            }
        } else {
            prompt.push_str("(unable to read file)\n");
        }

        prompt.push_str("\n\n");
    }

    if files.len() > MAX_FILES_PER_PROMPT {
        prompt.push_str(&format!(
            "({} more files not shown)\n",
            files.len() - MAX_FILES_PER_PROMPT
        ));
    }

    prompt
}

/// Build a prompt for analyzing git commit history.
///
/// Includes commit metadata (hash, message, diff stats) and asks
/// the model to identify decisions, migrations, and notable fixes.
pub fn build_commit_analysis_prompt(commits: &[CommitMetadata]) -> String {
    let mut prompt = String::from(
        "Analyze the following git commits from a codebase. \
         Identify architectural decisions, migrations, notable bug fixes, \
         and significant refactoring efforts.\n\n\
         Output your findings as TOML entries using this exact format:\n\n\
         ```\n\
         [[entry]]\n\
         what = \"one-sentence description of the decision or change\"\n\
         why = \"inferred reasoning based on commit message and context\"\n\
         how = \"what was changed and how it was implemented\"\n\n\
         [entry.context]\n\
         commits = [\"abc1234\"]\n\
         files = [\"affected/files.rs\"]\n\
         ```\n\n\
         Focus on commits that represent important decisions, breaking changes, \
         migrations, or lessons learned. Skip trivial commits.\n\n\
         --- COMMITS ---\n\n",
    );

    for commit in commits {
        prompt.push_str(&format!(
            "commit {} ({})\n  {}\n  {} files changed, +{} -{}\n\n",
            &commit.short_hash,
            commit.author,
            commit.message_summary,
            commit.files_changed,
            commit.insertions,
            commit.deletions,
        ));
    }

    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_file(path: &str, hash: &str, size: u64) -> FileToAnalyze {
        FileToAnalyze {
            path: path.to_string(),
            hash: hash.to_string(),
            size,
            is_new: true,
            is_changed: false,
        }
    }

    fn make_commit(hash: &str, message: &str) -> CommitMetadata {
        CommitMetadata {
            hash: hash.to_string(),
            short_hash: hash[..7.min(hash.len())].to_string(),
            author: "Test User <test@example.com>".to_string(),
            timestamp: 1700000000,
            message: message.to_string(),
            message_summary: message.to_string(),
            files_changed: 3,
            insertions: 42,
            deletions: 10,
            parent_hashes: vec![],
        }
    }

    #[test]
    fn test_file_analysis_prompt_contains_format_instructions() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("main.rs"), "fn main() {}").unwrap();

        let files = vec![make_file("main.rs", "abc123", 12)];
        let prompt = build_file_analysis_prompt(temp_dir.path(), &files);

        assert!(prompt.contains("[[entry]]"));
        assert!(prompt.contains("what ="));
        assert!(prompt.contains("why ="));
        assert!(prompt.contains("how ="));
        assert!(prompt.contains("main.rs"));
    }

    #[test]
    fn test_file_analysis_prompt_includes_content() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("main.rs"), "fn main() {\n    println!(\"hello\");\n}").unwrap();

        let files = vec![make_file("main.rs", "abc123", 40)];
        let prompt = build_file_analysis_prompt(temp_dir.path(), &files);

        assert!(prompt.contains("fn main()"));
        assert!(prompt.contains("println!"));
    }

    #[test]
    fn test_file_analysis_prompt_truncates_long_files() {
        let temp_dir = TempDir::new().unwrap();

        let long_content: String = (0..500)
            .map(|i| format!("line {}\n", i))
            .collect();
        fs::write(temp_dir.path().join("big.rs"), &long_content).unwrap();

        let files = vec![make_file("big.rs", "abc123", long_content.len() as u64)];
        let prompt = build_file_analysis_prompt(temp_dir.path(), &files);

        assert!(prompt.contains("more lines truncated"));
    }

    #[test]
    fn test_file_analysis_prompt_limits_file_count() {
        let temp_dir = TempDir::new().unwrap();

        let mut files = Vec::new();
        for i in 0..60 {
            let name = format!("file_{}.rs", i);
            fs::write(temp_dir.path().join(&name), "content").unwrap();
            files.push(make_file(&name, "abc", 7));
        }

        let prompt = build_file_analysis_prompt(temp_dir.path(), &files);

        assert!(prompt.contains("more files not shown"));
    }

    #[test]
    fn test_commit_analysis_prompt_contains_format_instructions() {
        let commits = vec![make_commit("abc1234def", "Add authentication module")];
        let prompt = build_commit_analysis_prompt(&commits);

        assert!(prompt.contains("[[entry]]"));
        assert!(prompt.contains("abc1234"));
        assert!(prompt.contains("Add authentication module"));
        assert!(prompt.contains("+42 -10"));
    }

    #[test]
    fn test_commit_analysis_prompt_multiple_commits() {
        let commits = vec![
            make_commit("abc1234def", "Refactor database layer"),
            make_commit("def5678abc", "Fix auth bypass vulnerability"),
        ];
        let prompt = build_commit_analysis_prompt(&commits);

        assert!(prompt.contains("Refactor database layer"));
        assert!(prompt.contains("Fix auth bypass vulnerability"));
    }
}
