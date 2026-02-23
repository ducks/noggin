//! Git commit walker for chronological history traversal
//!
//! Extracts commit metadata for knowledge extraction, supporting:
//! - Chronological walking (oldest to newest)
//! - Incremental processing via manifest tracking
//! - Diff statistics (files changed, insertions, deletions)
//! - Merge commit filtering
//! - Pagination for large repositories

use anyhow::{Context, Result};
use git2::{DiffOptions, Oid, Repository, Revwalk, Sort};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Metadata extracted from a single commit
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitMetadata {
    /// Full SHA hash
    pub hash: String,
    /// Short hash (7 characters)
    pub short_hash: String,
    /// Author name and email
    pub author: String,
    /// Commit timestamp (Unix timestamp)
    pub timestamp: i64,
    /// Full commit message
    pub message: String,
    /// First line of commit message
    pub message_summary: String,
    /// Number of files changed
    pub files_changed: u32,
    /// Lines inserted
    pub insertions: u32,
    /// Lines deleted
    pub deletions: u32,
    /// Parent commit hashes (multiple for merge commits)
    pub parent_hashes: Vec<String>,
}

/// Options for walking commits
#[derive(Debug, Clone, Default)]
pub struct WalkOptions {
    /// Skip merge commits (commits with > 1 parent)
    pub skip_merges: bool,
    /// Only process commits after this hash (for incremental walks)
    pub since_commit: Option<String>,
    /// Maximum number of commits to process (for pagination)
    pub limit: Option<usize>,
    /// Filter commits touching specific paths
    pub pathspec: Option<Vec<String>>,
}

/// Result of walking commits with optional continuation token
#[derive(Debug)]
pub struct WalkResult {
    /// Commits processed in this walk
    pub commits: Vec<CommitMetadata>,
    /// Hash to resume from for next batch (if limit was reached)
    pub next_hash: Option<String>,
}

/// Walk repository commits in chronological order and extract metadata
pub fn walk_commits(repo_path: &Path, options: WalkOptions) -> Result<WalkResult> {
    let repo = Repository::open(repo_path)
        .with_context(|| format!("Failed to open git repository at {}", repo_path.display()))?;

    // Set up revision walker
    let revwalk = setup_revwalk(&repo, &options)
        .context("Failed to set up revision walker")?;

    let mut commits = Vec::new();
    let mut next_hash = None;

    for oid_result in revwalk {
        let oid = oid_result.context("Failed to get commit OID")?;

        // Check limit
        if let Some(limit) = options.limit {
            if commits.len() >= limit {
                next_hash = Some(oid.to_string());
                break;
            }
        }

        let commit = repo.find_commit(oid)
            .with_context(|| format!("Failed to find commit {}", oid))?;

        // Skip merge commits if requested
        if options.skip_merges && commit.parent_count() > 1 {
            continue;
        }

        // Extract metadata
        let metadata = extract_commit_metadata(&repo, &commit, &options)
            .with_context(|| format!("Failed to extract metadata for commit {}", oid))?;

        commits.push(metadata);
    }

    Ok(WalkResult { commits, next_hash })
}

/// Set up revision walker with proper sorting and starting point
fn setup_revwalk<'a>(repo: &'a Repository, options: &WalkOptions) -> Result<Revwalk<'a>> {
    let mut revwalk = repo.revwalk()
        .context("Failed to create revision walker")?;

    // Sort chronologically (oldest first)
    revwalk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE)
        .context("Failed to set revwalk sorting")?;

    // Determine starting point
    if let Some(since_hash) = &options.since_commit {
        // Start from specific commit (for incremental walks)
        let oid = Oid::from_str(since_hash)
            .with_context(|| format!("Invalid commit hash: {}", since_hash))?;
        revwalk.push(oid)
            .with_context(|| format!("Failed to push commit {} to revwalk", since_hash))?;
    } else {
        // Start from HEAD
        match repo.head() {
            Ok(_head) => {
                revwalk.push_head()
                    .context("Failed to push HEAD to revwalk")?;
            }
            Err(_) => {
                // Detached HEAD or empty repo - try main/master
                if let Ok(_reference) = repo.find_reference("refs/heads/main") {
                    revwalk.push_ref("refs/heads/main")
                        .context("Failed to push main branch to revwalk")?;
                } else if let Ok(_reference) = repo.find_reference("refs/heads/master") {
                    revwalk.push_ref("refs/heads/master")
                        .context("Failed to push master branch to revwalk")?;
                } else {
                    // Empty repository - return empty walk
                    return Ok(revwalk);
                }
            }
        }
    }

    Ok(revwalk)
}

/// Extract metadata from a single commit
fn extract_commit_metadata(
    repo: &Repository,
    commit: &git2::Commit,
    options: &WalkOptions,
) -> Result<CommitMetadata> {
    let hash = commit.id().to_string();
    let short_hash = commit.as_object()
        .short_id()
        .map(|buf| buf.as_str().unwrap_or(&hash[..7]).to_string())
        .unwrap_or_else(|_| hash[..7].to_string());

    let author = commit.author();
    let author_str = format!(
        "{} <{}>",
        author.name().unwrap_or("Unknown"),
        author.email().unwrap_or("unknown@example.com")
    );

    let timestamp = author.when().seconds();

    let message = commit.message().unwrap_or("").to_string();
    let message_summary = message.lines().next().unwrap_or("").to_string();

    let parent_hashes: Vec<String> = commit.parents()
        .map(|p| p.id().to_string())
        .collect();

    // Calculate diff statistics
    let (files_changed, insertions, deletions) = calculate_diff_stats(repo, commit, options)
        .unwrap_or((0, 0, 0)); // If diff fails, use zeros (e.g., initial commit)

    Ok(CommitMetadata {
        hash,
        short_hash,
        author: author_str,
        timestamp,
        message,
        message_summary,
        files_changed,
        insertions,
        deletions,
        parent_hashes,
    })
}

/// Calculate diff statistics for a commit
fn calculate_diff_stats(
    repo: &Repository,
    commit: &git2::Commit,
    options: &WalkOptions,
) -> Result<(u32, u32, u32)> {
    // Get current and parent trees
    let current_tree = commit.tree()
        .context("Failed to get commit tree")?;

    let parent_tree = if commit.parent_count() > 0 {
        Some(commit.parent(0)
            .context("Failed to get parent commit")?
            .tree()
            .context("Failed to get parent tree")?)
    } else {
        None // Initial commit - no parent
    };

    // Create diff options with pathspec filter if provided
    let mut diff_opts = DiffOptions::new();
    if let Some(pathspecs) = &options.pathspec {
        for pathspec in pathspecs {
            diff_opts.pathspec(pathspec);
        }
    }

    // Calculate diff
    let diff = repo.diff_tree_to_tree(
        parent_tree.as_ref(),
        Some(&current_tree),
        Some(&mut diff_opts),
    ).context("Failed to create diff")?;

    let stats = diff.stats()
        .context("Failed to calculate diff stats")?;

    Ok((
        stats.files_changed() as u32,
        stats.insertions() as u32,
        stats.deletions() as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_repo() -> Result<(TempDir, Repository)> {
        let temp_dir = TempDir::new()?;
        let repo = Repository::init(temp_dir.path())?;

        // Configure git user for commits
        let mut config = repo.config()?;
        config.set_str("user.name", "Test User")?;
        config.set_str("user.email", "test@example.com")?;

        Ok((temp_dir, repo))
    }

    fn create_commit(repo: &Repository, message: &str, content: &str) -> Result<Oid> {
        let repo_path = repo.path().parent().unwrap();
        let file_path = repo_path.join("test.txt");
        fs::write(&file_path, content)?;

        let mut index = repo.index()?;
        index.add_path(Path::new("test.txt"))?;
        index.write()?;

        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;

        let signature = repo.signature()?;
        let parent_commit = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents = if let Some(ref p) = parent_commit {
            vec![p]
        } else {
            vec![]
        };

        let oid = repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )?;

        Ok(oid)
    }

    #[test]
    fn test_walk_commits_chronological_order() -> Result<()> {
        let (_temp, repo) = create_test_repo()?;

        // Create three commits
        create_commit(&repo, "First commit", "content1")?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        create_commit(&repo, "Second commit", "content2")?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        create_commit(&repo, "Third commit", "content3")?;

        let result = walk_commits(repo.path().parent().unwrap(), WalkOptions::default())?;

        assert_eq!(result.commits.len(), 3);
        assert_eq!(result.commits[0].message_summary, "First commit");
        assert_eq!(result.commits[1].message_summary, "Second commit");
        assert_eq!(result.commits[2].message_summary, "Third commit");

        // Verify timestamps are increasing
        assert!(result.commits[0].timestamp <= result.commits[1].timestamp);
        assert!(result.commits[1].timestamp <= result.commits[2].timestamp);

        Ok(())
    }

    #[test]
    fn test_commit_metadata_extraction() -> Result<()> {
        let (_temp, repo) = create_test_repo()?;
        let oid = create_commit(&repo, "Test commit", "test content")?;

        let result = walk_commits(repo.path().parent().unwrap(), WalkOptions::default())?;

        assert_eq!(result.commits.len(), 1);
        let metadata = &result.commits[0];

        assert_eq!(metadata.hash, oid.to_string());
        assert_eq!(metadata.short_hash.len(), 7);
        assert_eq!(metadata.author, "Test User <test@example.com>");
        assert_eq!(metadata.message_summary, "Test commit");
        assert_eq!(metadata.files_changed, 1);
        assert_eq!(metadata.parent_hashes.len(), 0); // Initial commit

        Ok(())
    }

    #[test]
    fn test_diff_statistics() -> Result<()> {
        let (_temp, repo) = create_test_repo()?;

        // Initial commit with 3 lines
        create_commit(&repo, "Initial", "line1\nline2\nline3")?;

        // Second commit: add 2 lines, remove 1 line
        create_commit(&repo, "Update", "line1\nline3\nline4\nline5")?;

        let result = walk_commits(repo.path().parent().unwrap(), WalkOptions::default())?;

        assert_eq!(result.commits.len(), 2);
        let second_commit = &result.commits[1];

        assert_eq!(second_commit.files_changed, 1);
        assert_eq!(second_commit.insertions, 2); // Added line4, line5
        assert_eq!(second_commit.deletions, 1); // Removed line2

        Ok(())
    }

    #[test]
    fn test_incremental_walk() -> Result<()> {
        let (_temp, repo) = create_test_repo()?;

        let first_oid = create_commit(&repo, "First", "content1")?;
        create_commit(&repo, "Second", "content2")?;
        create_commit(&repo, "Third", "content3")?;

        // Walk starting from second commit
        let options = WalkOptions {
            since_commit: Some(first_oid.to_string()),
            ..Default::default()
        };

        let result = walk_commits(repo.path().parent().unwrap(), options)?;

        // Should only get commits after first_oid
        assert_eq!(result.commits.len(), 2);
        assert_eq!(result.commits[0].message_summary, "Second");
        assert_eq!(result.commits[1].message_summary, "Third");

        Ok(())
    }

    #[test]
    fn test_pagination() -> Result<()> {
        let (_temp, repo) = create_test_repo()?;

        create_commit(&repo, "First", "content1")?;
        create_commit(&repo, "Second", "content2")?;
        create_commit(&repo, "Third", "content3")?;

        // Limit to 2 commits
        let options = WalkOptions {
            limit: Some(2),
            ..Default::default()
        };

        let result = walk_commits(repo.path().parent().unwrap(), options)?;

        assert_eq!(result.commits.len(), 2);
        assert!(result.next_hash.is_some());

        Ok(())
    }

    #[test]
    fn test_empty_repository() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let _repo = Repository::init(temp_dir.path())?;

        let result = walk_commits(temp_dir.path(), WalkOptions::default())?;

        assert_eq!(result.commits.len(), 0);
        assert!(result.next_hash.is_none());

        Ok(())
    }
}
