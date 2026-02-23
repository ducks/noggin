use anyhow::Result;
use llm_noggin::git::walker::{walk_commits, WalkOptions};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn create_test_repo_with_history() -> Result<TempDir> {
    let temp_dir = TempDir::new()?;
    let repo = git2::Repository::init(temp_dir.path())?;

    // Configure git
    let mut config = repo.config()?;
    config.set_str("user.name", "Test User")?;
    config.set_str("user.email", "test@example.com")?;

    // Create multiple commits
    for i in 1..=5 {
        let file_path = temp_dir.path().join(format!("file{}.txt", i));
        fs::write(&file_path, format!("content {}", i))?;

        let mut index = repo.index()?;
        index.add_path(Path::new(&format!("file{}.txt", i)))?;
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

        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            &format!("Commit {}", i),
            &tree,
            &parents,
        )?;

        // Small delay to ensure different timestamps
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    Ok(temp_dir)
}

#[test]
fn test_walk_returns_commits_in_chronological_order() -> Result<()> {
    let temp_dir = create_test_repo_with_history()?;
    let result = walk_commits(temp_dir.path(), WalkOptions::default())?;

    assert_eq!(result.commits.len(), 5);

    // Verify chronological order (oldest first)
    for i in 0..4 {
        assert!(result.commits[i].timestamp <= result.commits[i + 1].timestamp);
    }

    // Verify commit messages
    for (i, commit) in result.commits.iter().enumerate() {
        assert_eq!(commit.message_summary, format!("Commit {}", i + 1));
    }

    Ok(())
}

#[test]
fn test_diff_stats_match_expected_values() -> Result<()> {
    let temp_dir = create_test_repo_with_history()?;
    let result = walk_commits(temp_dir.path(), WalkOptions::default())?;

    // Each commit adds one new file with one line
    for commit in &result.commits {
        assert_eq!(commit.files_changed, 1);
        assert_eq!(commit.insertions, 1);
        assert_eq!(commit.deletions, 0);
    }

    Ok(())
}

#[test]
fn test_incremental_walk_from_middle_commit() -> Result<()> {
    let temp_dir = create_test_repo_with_history()?;

    // First, get all commits
    let all_commits = walk_commits(temp_dir.path(), WalkOptions::default())?;
    assert_eq!(all_commits.commits.len(), 5);

    // Get hash of third commit
    let third_commit_hash = all_commits.commits[2].hash.clone();

    // Walk from third commit onward
    let options = WalkOptions {
        since_commit: Some(third_commit_hash),
        ..Default::default()
    };
    let incremental = walk_commits(temp_dir.path(), options)?;

    // Should get commits 4 and 5
    assert_eq!(incremental.commits.len(), 2);
    assert_eq!(incremental.commits[0].message_summary, "Commit 4");
    assert_eq!(incremental.commits[1].message_summary, "Commit 5");

    Ok(())
}

#[test]
fn test_pagination_with_limit() -> Result<()> {
    let temp_dir = create_test_repo_with_history()?;

    // Request first 3 commits
    let options = WalkOptions {
        limit: Some(3),
        ..Default::default()
    };
    let result = walk_commits(temp_dir.path(), options)?;

    assert_eq!(result.commits.len(), 3);
    assert!(result.next_hash.is_some());

    // Verify we got the first 3 commits
    for i in 0..3 {
        assert_eq!(result.commits[i].message_summary, format!("Commit {}", i + 1));
    }

    Ok(())
}

#[test]
fn test_skip_merges_option() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let repo = git2::Repository::init(temp_dir.path())?;

    let mut config = repo.config()?;
    config.set_str("user.name", "Test User")?;
    config.set_str("user.email", "test@example.com")?;

    // Create a simple commit
    let file_path = temp_dir.path().join("file.txt");
    fs::write(&file_path, "content")?;

    let mut index = repo.index()?;
    index.add_path(Path::new("file.txt"))?;
    index.write()?;

    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let signature = repo.signature()?;

    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "Initial commit",
        &tree,
        &[],
    )?;

    // For this test, we just verify the option doesn't break anything
    // Creating actual merge commits in tests is complex
    let options = WalkOptions {
        skip_merges: true,
        ..Default::default()
    };

    let result = walk_commits(temp_dir.path(), options)?;
    assert_eq!(result.commits.len(), 1);

    Ok(())
}

#[test]
fn test_empty_repository() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let _repo = git2::Repository::init(temp_dir.path())?;

    let result = walk_commits(temp_dir.path(), WalkOptions::default())?;

    assert_eq!(result.commits.len(), 0);
    assert!(result.next_hash.is_none());

    Ok(())
}

#[test]
fn test_repository_not_found_error() {
    let temp_dir = TempDir::new().unwrap();
    let non_git_path = temp_dir.path().join("not-a-repo");
    fs::create_dir(&non_git_path).unwrap();

    let result = walk_commits(&non_git_path, WalkOptions::default());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Failed to open git repository"));
}
