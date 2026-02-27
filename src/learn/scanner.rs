//! File discovery and hash-based change detection.
//!
//! Walks the repository, calculates SHA-256 hashes, and compares against
//! the manifest to identify files that need analysis.

use crate::manifest::{calculate_file_hash, Manifest};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

/// A file identified for analysis
#[derive(Debug, Clone)]
pub struct FileToAnalyze {
    /// Relative path from repo root
    pub path: String,
    /// SHA-256 hash of file contents
    pub hash: String,
    /// File size in bytes
    pub size: u64,
    /// True if file is not tracked in manifest
    pub is_new: bool,
    /// True if file hash differs from manifest
    pub is_changed: bool,
}

/// Result of scanning the repository
#[derive(Debug)]
pub struct ScanResult {
    /// Files that need analysis (new or changed)
    pub changed: Vec<FileToAnalyze>,
    /// Number of unchanged files skipped
    pub unchanged: usize,
    /// Total files examined
    pub total: usize,
}

/// Scan repository for files needing analysis.
///
/// Walks the repo, skips ignored/binary files, calculates hashes,
/// and compares against manifest to find changed files.
/// If `full` is true, all files are returned regardless of manifest state.
pub fn scan_files(repo_path: &Path, manifest: &Manifest, full: bool) -> Result<ScanResult> {
    let repo = git2::Repository::open(repo_path)
        .with_context(|| format!("Failed to open git repository at {}", repo_path.display()))?;

    let mut changed = Vec::new();
    let mut unchanged = 0usize;
    let mut total = 0usize;

    for entry in WalkDir::new(repo_path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip .git and .noggin directories at walk level
            name != ".git" && name != ".noggin"
        })
    {
        let entry = entry.context("Failed to read directory entry")?;

        if !entry.file_type().is_file() {
            continue;
        }

        let full_path = entry.path();

        // Get relative path
        let rel_path = match full_path.strip_prefix(repo_path) {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(_) => continue,
        };

        // Skip files ignored by git
        if repo.is_path_ignored(Path::new(&rel_path)).unwrap_or(false) {
            continue;
        }

        // Skip binary files (check first 512 bytes for null bytes)
        if is_binary(full_path) {
            continue;
        }

        total += 1;

        // Calculate hash
        let hash = calculate_file_hash(full_path)
            .with_context(|| format!("Failed to hash {}", rel_path))?;

        let metadata = fs::metadata(full_path)
            .with_context(|| format!("Failed to read metadata for {}", rel_path))?;

        if full {
            // In full mode, analyze everything
            let is_new = manifest.get_file_hash(&rel_path).is_none();
            changed.push(FileToAnalyze {
                path: rel_path,
                hash,
                size: metadata.len(),
                is_new,
                is_changed: true,
            });
        } else if manifest.is_file_changed(&rel_path, &hash) {
            let is_new = manifest.get_file_hash(&rel_path).is_none();
            changed.push(FileToAnalyze {
                path: rel_path,
                hash,
                size: metadata.len(),
                is_new,
                is_changed: !is_new,
            });
        } else {
            unchanged += 1;
        }
    }

    Ok(ScanResult {
        changed,
        unchanged,
        total,
    })
}

/// Check if a file is binary by looking for null bytes in the first 512 bytes.
fn is_binary(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    let check_len = bytes.len().min(512);
    bytes[..check_len].contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_repo() -> Result<(TempDir, git2::Repository)> {
        let temp_dir = TempDir::new()?;
        let repo = git2::Repository::init(temp_dir.path())?;

        let mut config = repo.config()?;
        config.set_str("user.name", "Test User")?;
        config.set_str("user.email", "test@example.com")?;

        Ok((temp_dir, repo))
    }

    #[test]
    fn test_scan_finds_new_files() -> Result<()> {
        let (temp_dir, _repo) = create_test_repo()?;

        fs::write(temp_dir.path().join("hello.rs"), "fn main() {}")?;
        fs::write(temp_dir.path().join("lib.rs"), "pub fn add() {}")?;

        let manifest = Manifest::default();
        let result = scan_files(temp_dir.path(), &manifest, false)?;

        assert_eq!(result.total, 2);
        assert_eq!(result.changed.len(), 2);
        assert_eq!(result.unchanged, 0);
        assert!(result.changed.iter().all(|f| f.is_new));

        Ok(())
    }

    #[test]
    fn test_scan_skips_unchanged_files() -> Result<()> {
        let (temp_dir, _repo) = create_test_repo()?;

        let content = "fn main() {}";
        fs::write(temp_dir.path().join("hello.rs"), content)?;

        let hash = calculate_file_hash(&temp_dir.path().join("hello.rs"))?;
        let mut manifest = Manifest::default();
        manifest.add_or_update_file("hello.rs".to_string(), hash, vec![]);

        let result = scan_files(temp_dir.path(), &manifest, false)?;

        assert_eq!(result.total, 1);
        assert_eq!(result.changed.len(), 0);
        assert_eq!(result.unchanged, 1);

        Ok(())
    }

    #[test]
    fn test_scan_detects_changed_files() -> Result<()> {
        let (temp_dir, _repo) = create_test_repo()?;

        fs::write(temp_dir.path().join("hello.rs"), "fn main() {}")?;

        let mut manifest = Manifest::default();
        manifest.add_or_update_file(
            "hello.rs".to_string(),
            "old_hash".to_string(),
            vec![],
        );

        let result = scan_files(temp_dir.path(), &manifest, false)?;

        assert_eq!(result.changed.len(), 1);
        assert!(result.changed[0].is_changed);
        assert!(!result.changed[0].is_new);

        Ok(())
    }

    #[test]
    fn test_scan_full_mode_includes_all() -> Result<()> {
        let (temp_dir, _repo) = create_test_repo()?;

        let content = "fn main() {}";
        fs::write(temp_dir.path().join("hello.rs"), content)?;

        let hash = calculate_file_hash(&temp_dir.path().join("hello.rs"))?;
        let mut manifest = Manifest::default();
        manifest.add_or_update_file("hello.rs".to_string(), hash, vec![]);

        // Even though file is unchanged, --full should include it
        let result = scan_files(temp_dir.path(), &manifest, true)?;

        assert_eq!(result.changed.len(), 1);

        Ok(())
    }

    #[test]
    fn test_scan_skips_git_directory() -> Result<()> {
        let (temp_dir, _repo) = create_test_repo()?;

        fs::write(temp_dir.path().join("hello.rs"), "fn main() {}")?;

        let manifest = Manifest::default();
        let result = scan_files(temp_dir.path(), &manifest, false)?;

        // Should not include any .git/ files
        assert!(result.changed.iter().all(|f| !f.path.starts_with(".git")));

        Ok(())
    }

    #[test]
    fn test_scan_skips_binary_files() -> Result<()> {
        let (temp_dir, _repo) = create_test_repo()?;

        // Create a text file
        fs::write(temp_dir.path().join("hello.rs"), "fn main() {}")?;

        // Create a binary file with null bytes
        let mut binary = fs::File::create(temp_dir.path().join("image.png"))?;
        binary.write_all(&[0x89, 0x50, 0x4E, 0x47, 0x00, 0x00])?;

        let manifest = Manifest::default();
        let result = scan_files(temp_dir.path(), &manifest, false)?;

        assert_eq!(result.changed.len(), 1);
        assert_eq!(result.changed[0].path, "hello.rs");

        Ok(())
    }

    #[test]
    fn test_is_binary() {
        let temp_dir = TempDir::new().unwrap();

        let text_path = temp_dir.path().join("text.rs");
        fs::write(&text_path, "fn main() {}").unwrap();
        assert!(!is_binary(&text_path));

        let binary_path = temp_dir.path().join("binary.bin");
        fs::write(&binary_path, &[0x00, 0x01, 0x02]).unwrap();
        assert!(is_binary(&binary_path));
    }

    #[test]
    fn test_scan_skips_gitignored_files() -> Result<()> {
        let (temp_dir, _repo) = create_test_repo()?;

        fs::write(temp_dir.path().join(".gitignore"), "target/\n*.log\n")?;
        fs::create_dir_all(temp_dir.path().join("target"))?;
        fs::write(temp_dir.path().join("target/debug.bin"), "binary stuff")?;
        fs::write(temp_dir.path().join("app.log"), "log output")?;
        fs::write(temp_dir.path().join("hello.rs"), "fn main() {}")?;

        let manifest = Manifest::default();
        let result = scan_files(temp_dir.path(), &manifest, false)?;

        let paths: Vec<&str> = result.changed.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"hello.rs"));
        assert!(paths.contains(&".gitignore"));
        assert!(!paths.contains(&"app.log"));
        assert!(!paths.iter().any(|p| p.starts_with("target")));

        Ok(())
    }
}
