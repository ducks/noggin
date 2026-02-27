use git2::Repository;
use llm_noggin::git::scoring::{score_commit, ScoreCategory, ScoringConfig};
use std::path::Path;
use tempfile::TempDir;

fn create_test_repo() -> (TempDir, Repository) {
    let dir = TempDir::new().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    
    let mut config = repo.config().unwrap();
    config.set_str("user.name", "Test User").unwrap();
    config.set_str("user.email", "test@example.com").unwrap();
    
    (dir, repo)
}

fn create_commit(
    repo: &Repository,
    path: &str,
    content: &str,
    message: &str,
) -> git2::Oid {
    let tree_id = {
        let mut index = repo.index().unwrap();
        let repo_path = repo.path().parent().unwrap();
        let file_path = repo_path.join(path);
        
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        
        std::fs::write(&file_path, content).unwrap();
        index.add_path(Path::new(path)).unwrap();
        index.write().unwrap();
        index.write_tree().unwrap()
    };
    
    let tree = repo.find_tree(tree_id).unwrap();
    let sig = repo.signature().unwrap();
    
    let parent_commit = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let parents = if let Some(ref p) = parent_commit {
        vec![p]
    } else {
        vec![]
    };
    
    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        message,
        &tree,
        &parents,
    )
    .unwrap()
}

#[test]
fn test_score_small_diff() {
    let (_dir, repo) = create_test_repo();
    let config = ScoringConfig::default();
    
    let oid = create_commit(&repo, "test.txt", "hello\n", "Add test file");
    let commit = repo.find_commit(oid).unwrap();
    
    let score = score_commit(&repo, &commit, &config).unwrap();
    
    assert!(score.significance < 0.5, "Small diff should have low score");
}

#[test]
fn test_score_migration_file() {
    let (_dir, repo) = create_test_repo();
    let config = ScoringConfig::default();
    
    let content = "ALTER TABLE users ADD COLUMN email VARCHAR(255);\n".repeat(10);
    let oid = create_commit(
        &repo,
        "migrations/20260223_add_email.sql",
        &content,
        "Add email column migration",
    );
    let commit = repo.find_commit(oid).unwrap();
    
    let score = score_commit(&repo, &commit, &config).unwrap();
    
    assert!(
        score.significance > 0.4,
        "Migration should have high score, got {}",
        score.significance
    );
}

#[test]
fn test_score_breaking_change() {
    let (_dir, repo) = create_test_repo();
    let config = ScoringConfig::default();
    
    let oid = create_commit(
        &repo,
        "src/api.rs",
        &"pub fn new_api() {}".repeat(20),
        "BREAKING CHANGE: Remove old API endpoints",
    );
    let commit = repo.find_commit(oid).unwrap();
    
    let score = score_commit(&repo, &commit, &config).unwrap();
    
    assert_eq!(
        score.category,
        ScoreCategory::Critical,
        "Breaking change should be critical"
    );
}

#[test]
fn test_score_typo_fix() {
    let (_dir, repo) = create_test_repo();
    let config = ScoringConfig::default();
    
    let oid = create_commit(
        &repo,
        "README.md",
        "# Project\n\nThis is a typo fix.\n",
        "Fix typo in README",
    );
    let commit = repo.find_commit(oid).unwrap();
    
    let score = score_commit(&repo, &commit, &config).unwrap();
    
    assert!(
        score.category == ScoreCategory::Trivial || score.category == ScoreCategory::Low,
        "Typo fix should be trivial/low, got {:?}",
        score.category
    );
}

#[test]
fn test_score_large_refactor() {
    let (_dir, repo) = create_test_repo();
    let config = ScoringConfig::default();
    
    let content = "fn refactored_function() {\n    // New implementation\n}\n".repeat(50);
    let oid = create_commit(
        &repo,
        "src/core/engine.rs",
        &content,
        "Refactor core engine for better performance",
    );
    let commit = repo.find_commit(oid).unwrap();
    
    let score = score_commit(&repo, &commit, &config).unwrap();
    
    assert!(
        matches!(score.category, ScoreCategory::High | ScoreCategory::Critical),
        "Large refactor should be high/critical, got {:?}",
        score.category
    );
}

#[test]
fn test_score_factors() {
    let (_dir, repo) = create_test_repo();
    let config = ScoringConfig::default();
    
    let oid = create_commit(
        &repo,
        "migrations/init.sql",
        &"CREATE TABLE users;\n".repeat(20),
        "Add initial migration",
    );
    let commit = repo.find_commit(oid).unwrap();
    
    let score = score_commit(&repo, &commit, &config).unwrap();
    
    assert!(
        !score.factors.is_empty(),
        "Score should have factors explaining the score"
    );
}

#[test]
fn test_score_category_conversion() {
    assert_eq!(ScoreCategory::from_score(0.95), ScoreCategory::Critical);
    assert_eq!(ScoreCategory::from_score(0.75), ScoreCategory::High);
    assert_eq!(ScoreCategory::from_score(0.50), ScoreCategory::Medium);
    assert_eq!(ScoreCategory::from_score(0.25), ScoreCategory::Low);
    assert_eq!(ScoreCategory::from_score(0.05), ScoreCategory::Trivial);
}
