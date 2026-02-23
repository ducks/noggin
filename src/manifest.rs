use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    #[serde(default)]
    pub files: HashMap<String, FileEntry>,
    #[serde(default)]
    pub commits: HashMap<String, CommitEntry>,
    #[serde(default)]
    pub patterns: HashMap<String, PatternEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub hash: String,
    pub last_scanned: DateTime<Utc>,
    #[serde(default)]
    pub pattern_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitEntry {
    pub sha: String,
    pub processed_at: DateTime<Utc>,
    pub category: CommitCategory,
    pub arf_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommitCategory {
    Decision,
    Migration,
    Bug,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub contributing_files: Vec<String>,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ManifestStats {
    pub files_scanned: usize,
    pub commits_processed: usize,
    pub patterns_extracted: usize,
    pub last_scan: Option<DateTime<Utc>>,
}

impl Manifest {
    /// Load manifest from file, returns empty manifest if file doesn't exist
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read manifest from {}", path.display()))?;

        toml::from_str(&contents)
            .with_context(|| format!("Failed to parse manifest from {}", path.display()))
    }

    /// Save manifest to file atomically
    pub fn save(&self, path: &Path) -> Result<()> {
        let contents = toml::to_string_pretty(self)
            .context("Failed to serialize manifest to TOML")?;

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }

        // Write atomically: write to temp file, then rename
        let temp_path = path.with_extension("toml.tmp");
        fs::write(&temp_path, contents)
            .with_context(|| format!("Failed to write temp manifest to {}", temp_path.display()))?;

        fs::rename(&temp_path, path)
            .with_context(|| format!("Failed to rename temp manifest to {}", path.display()))?;

        Ok(())
    }

    /// Add or update a file entry
    pub fn add_or_update_file(&mut self, path: String, hash: String, pattern_ids: Vec<String>) {
        let entry = FileEntry {
            path: path.clone(),
            hash,
            last_scanned: Utc::now(),
            pattern_ids,
        };
        self.files.insert(path, entry);
    }

    /// Get file hash if tracked
    pub fn get_file_hash(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(|entry| entry.hash.as_str())
    }

    /// Check if file has changed compared to tracked hash
    pub fn is_file_changed(&self, path: &str, current_hash: &str) -> bool {
        match self.get_file_hash(path) {
            Some(tracked_hash) => tracked_hash != current_hash,
            None => true, // Not tracked = changed
        }
    }

    /// Add a processed commit
    pub fn add_commit(&mut self, sha: String, category: CommitCategory, arf_path: String) {
        let entry = CommitEntry {
            sha: sha.clone(),
            processed_at: Utc::now(),
            category,
            arf_path,
        };
        self.commits.insert(sha, entry);
    }

    /// Check if commit has been processed
    pub fn is_commit_processed(&self, sha: &str) -> bool {
        self.commits.contains_key(sha)
    }

    /// Get all commits processed after the given SHA (chronologically)
    pub fn get_commits_since(&self, sha: &str) -> Vec<&CommitEntry> {
        let target_timestamp = match self.commits.get(sha) {
            Some(entry) => entry.processed_at,
            None => return Vec::new(),
        };

        let mut commits: Vec<&CommitEntry> = self
            .commits
            .values()
            .filter(|entry| entry.processed_at > target_timestamp)
            .collect();

        commits.sort_by_key(|entry| entry.processed_at);
        commits
    }

    /// Link a pattern to a contributing file
    pub fn link_pattern_to_file(&mut self, pattern_id: &str, file_path: &str) {
        // Add pattern_id to file's pattern list
        if let Some(file_entry) = self.files.get_mut(file_path) {
            if !file_entry.pattern_ids.contains(&pattern_id.to_string()) {
                file_entry.pattern_ids.push(pattern_id.to_string());
            }
        }

        // Add file to pattern's contributing_files list
        if let Some(pattern_entry) = self.patterns.get_mut(pattern_id) {
            if !pattern_entry.contributing_files.contains(&file_path.to_string()) {
                pattern_entry.contributing_files.push(file_path.to_string());
            }
        }
    }

    /// Get all patterns associated with a file
    pub fn get_patterns_for_file(&self, path: &str) -> Vec<String> {
        self.files
            .get(path)
            .map(|entry| entry.pattern_ids.clone())
            .unwrap_or_default()
    }

    /// Mark pattern for re-analysis by updating its timestamp
    pub fn invalidate_pattern(&mut self, pattern_id: &str) {
        if let Some(pattern_entry) = self.patterns.get_mut(pattern_id) {
            pattern_entry.last_updated = Utc::now();
        }
    }

    /// Add or update a pattern entry
    pub fn add_or_update_pattern(&mut self, id: String, name: String, contributing_files: Vec<String>) {
        let entry = PatternEntry {
            id: id.clone(),
            name,
            contributing_files,
            last_updated: Utc::now(),
        };
        self.patterns.insert(id, entry);
    }

    /// Get manifest statistics
    pub fn stats(&self) -> ManifestStats {
        let last_scan = self
            .files
            .values()
            .map(|entry| entry.last_scanned)
            .max();

        ManifestStats {
            files_scanned: self.files.len(),
            commits_processed: self.commits.len(),
            patterns_extracted: self.patterns.len(),
            last_scan,
        }
    }
}

/// Calculate SHA-256 hash of a file
pub fn calculate_file_hash(path: &Path) -> Result<String> {
    let contents = fs::read(path)
        .with_context(|| format!("Failed to read file for hashing: {}", path.display()))?;

    let mut hasher = Sha256::new();
    hasher.update(&contents);
    let result = hasher.finalize();

    Ok(format!("{:x}", result))
}

/// Detect files that have changed since last scan
pub fn detect_file_changes(manifest: &Manifest, repo_path: &Path) -> Result<Vec<PathBuf>> {
    let mut changed_files = Vec::new();

    for (path_str, entry) in &manifest.files {
        let full_path = repo_path.join(path_str);

        if !full_path.exists() {
            // File was deleted
            changed_files.push(PathBuf::from(path_str));
            continue;
        }

        let current_hash = calculate_file_hash(&full_path)
            .with_context(|| format!("Failed to hash file: {}", full_path.display()))?;

        if current_hash != entry.hash {
            changed_files.push(PathBuf::from(path_str));
        }
    }

    Ok(changed_files)
}

/// Detect new commits since last processed commit
/// Returns vector of commit SHAs (not full Commit objects due to lifetime issues)
pub fn detect_new_commits(manifest: &Manifest, repo_path: &Path) -> Result<Vec<String>> {
    let repo = git2::Repository::open(repo_path)
        .with_context(|| format!("Failed to open git repository at {}", repo_path.display()))?;

    let mut revwalk = repo.revwalk()
        .context("Failed to create revision walker")?;

    revwalk.push_head()
        .context("Failed to push HEAD to revwalk")?;

    let mut new_commits = Vec::new();

    for oid in revwalk {
        let oid = oid.context("Failed to get commit OID")?;
        let sha = oid.to_string();

        if manifest.is_commit_processed(&sha) {
            // Found a processed commit, stop here
            break;
        }

        new_commits.push(sha);
    }

    // Reverse to get chronological order (oldest first)
    new_commits.reverse();

    Ok(new_commits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_calculate_file_hash() {
        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "hello world").unwrap();

        let hash = calculate_file_hash(temp_file.path()).unwrap();

        // SHA-256 of "hello world"
        assert_eq!(hash, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn test_manifest_serialization_roundtrip() {
        let mut manifest = Manifest::default();
        manifest.add_or_update_file(
            "src/main.rs".to_string(),
            "abc123".to_string(),
            vec!["pattern1".to_string()],
        );
        manifest.add_commit(
            "commit123".to_string(),
            CommitCategory::Decision,
            "decisions/test.arf".to_string(),
        );

        let toml = toml::to_string_pretty(&manifest).unwrap();
        let deserialized: Manifest = toml::from_str(&toml).unwrap();

        assert_eq!(deserialized.files.len(), 1);
        assert_eq!(deserialized.commits.len(), 1);
    }

    #[test]
    fn test_is_file_changed() {
        let mut manifest = Manifest::default();
        manifest.add_or_update_file(
            "src/main.rs".to_string(),
            "abc123".to_string(),
            vec![],
        );

        assert!(!manifest.is_file_changed("src/main.rs", "abc123"));
        assert!(manifest.is_file_changed("src/main.rs", "different"));
        assert!(manifest.is_file_changed("nonexistent.rs", "abc123"));
    }

    #[test]
    fn test_commit_tracking() {
        let mut manifest = Manifest::default();
        manifest.add_commit(
            "commit1".to_string(),
            CommitCategory::Bug,
            "bugs/fix.arf".to_string(),
        );

        assert!(manifest.is_commit_processed("commit1"));
        assert!(!manifest.is_commit_processed("commit2"));
    }

    #[test]
    fn test_pattern_invalidation() {
        let mut manifest = Manifest::default();
        manifest.add_or_update_pattern(
            "pattern1".to_string(),
            "Error Handling".to_string(),
            vec!["src/main.rs".to_string()],
        );

        let original_time = manifest.patterns.get("pattern1").unwrap().last_updated;

        // Wait a bit to ensure timestamp changes
        std::thread::sleep(std::time::Duration::from_millis(10));

        manifest.invalidate_pattern("pattern1");

        let updated_time = manifest.patterns.get("pattern1").unwrap().last_updated;
        assert!(updated_time > original_time);
    }

    #[test]
    fn test_link_pattern_to_file() {
        let mut manifest = Manifest::default();
        manifest.add_or_update_file(
            "src/main.rs".to_string(),
            "abc123".to_string(),
            vec![],
        );
        manifest.add_or_update_pattern(
            "pattern1".to_string(),
            "Error Handling".to_string(),
            vec![],
        );

        manifest.link_pattern_to_file("pattern1", "src/main.rs");

        let patterns = manifest.get_patterns_for_file("src/main.rs");
        assert_eq!(patterns, vec!["pattern1"]);

        let pattern = manifest.patterns.get("pattern1").unwrap();
        assert!(pattern.contributing_files.contains(&"src/main.rs".to_string()));
    }

    #[test]
    fn test_manifest_stats() {
        let mut manifest = Manifest::default();
        manifest.add_or_update_file(
            "src/main.rs".to_string(),
            "abc123".to_string(),
            vec![],
        );
        manifest.add_commit(
            "commit1".to_string(),
            CommitCategory::Decision,
            "decisions/test.arf".to_string(),
        );
        manifest.add_or_update_pattern(
            "pattern1".to_string(),
            "Error Handling".to_string(),
            vec![],
        );

        let stats = manifest.stats();
        assert_eq!(stats.files_scanned, 1);
        assert_eq!(stats.commits_processed, 1);
        assert_eq!(stats.patterns_extracted, 1);
        assert!(stats.last_scan.is_some());
    }

    #[test]
    fn test_load_nonexistent_manifest() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manifest_path = temp_dir.path().join("manifest.toml");

        let manifest = Manifest::load(&manifest_path).unwrap();
        assert_eq!(manifest.files.len(), 0);
        assert_eq!(manifest.commits.len(), 0);
    }

    #[test]
    fn test_save_and_load_manifest() {
        let temp_dir = tempfile::tempdir().unwrap();
        let manifest_path = temp_dir.path().join("manifest.toml");

        let mut manifest = Manifest::default();
        manifest.add_or_update_file(
            "src/main.rs".to_string(),
            "abc123".to_string(),
            vec!["pattern1".to_string()],
        );

        manifest.save(&manifest_path).unwrap();

        let loaded = Manifest::load(&manifest_path).unwrap();
        assert_eq!(loaded.files.len(), 1);
        assert_eq!(loaded.get_file_hash("src/main.rs"), Some("abc123"));
    }
}
