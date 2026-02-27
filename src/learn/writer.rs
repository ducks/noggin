//! ARF file writer for the .noggin/ knowledge base.
//!
//! Takes synthesized ARF files, infers their category, generates
//! filenames, and writes them to the appropriate subdirectory.

use crate::arf::ArfFile;
use crate::synthesis::merger::{infer_category, ArfCategory};
use anyhow::{Context, Result};
use std::path::Path;

/// Result of writing ARF files
#[derive(Debug)]
pub struct WriteResult {
    /// Number of new ARF files written
    pub written: usize,
    /// Number of existing ARF files updated
    pub updated: usize,
    /// Number of unchanged ARF files skipped
    pub skipped: usize,
}

/// Write ARF files to the appropriate .noggin/ subdirectories.
///
/// For each ARF, infers the category (decisions/patterns/bugs/migrations/facts),
/// generates a filename from the `what` field, and writes the TOML file.
/// Skips writing if an identical file already exists.
pub fn write_arfs(noggin_path: &Path, arfs: &[ArfFile]) -> Result<WriteResult> {
    let mut written = 0;
    let mut updated = 0;
    let mut skipped = 0;

    for arf in arfs {
        let category_dir = category_dirname(&infer_category(arf));
        let filename = slugify(&arf.what);
        let file_path = noggin_path.join(category_dir).join(format!("{}.arf", filename));

        // Check if identical file already exists
        if file_path.exists() {
            if let Ok(existing) = ArfFile::from_toml(&file_path) {
                if existing == *arf {
                    skipped += 1;
                    continue;
                }
                // File exists but content changed
                arf.to_toml(&file_path)
                    .with_context(|| format!("Failed to update {}", file_path.display()))?;
                updated += 1;
                continue;
            }
        }

        // Write new file
        arf.to_toml(&file_path)
            .with_context(|| format!("Failed to write {}", file_path.display()))?;
        written += 1;
    }

    Ok(WriteResult {
        written,
        updated,
        skipped,
    })
}

/// Map ArfCategory to subdirectory name
fn category_dirname(category: &ArfCategory) -> &'static str {
    match category {
        ArfCategory::Decision => "decisions",
        ArfCategory::Pattern => "patterns",
        ArfCategory::Bug => "bugs",
        ArfCategory::Migration => "migrations",
        ArfCategory::Fact => "facts",
    }
}

/// Convert a `what` field to a filename-safe slug.
///
/// Lowercases, replaces non-alphanumeric with hyphens, collapses
/// multiple hyphens, trims leading/trailing hyphens, truncates to 50 chars.
fn slugify(text: &str) -> String {
    let slug: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse multiple hyphens and trim
    let mut result = String::new();
    let mut prev_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen && !result.is_empty() {
                result.push('-');
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }

    // Trim trailing hyphen and truncate
    let trimmed = result.trim_end_matches('-');
    if trimmed.len() > 50 {
        // Find a clean break point
        let truncated = &trimmed[..50];
        truncated
            .rfind('-')
            .map(|i| &truncated[..i])
            .unwrap_or(truncated)
            .to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_noggin_dir() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let noggin = temp_dir.path();
        std::fs::create_dir_all(noggin.join("decisions")).unwrap();
        std::fs::create_dir_all(noggin.join("patterns")).unwrap();
        std::fs::create_dir_all(noggin.join("bugs")).unwrap();
        std::fs::create_dir_all(noggin.join("migrations")).unwrap();
        std::fs::create_dir_all(noggin.join("facts")).unwrap();
        temp_dir
    }

    #[test]
    fn test_slugify_basic() {
        assert_eq!(slugify("Use connection pooling"), "use-connection-pooling");
    }

    #[test]
    fn test_slugify_special_chars() {
        assert_eq!(
            slugify("Error handling (via anyhow)"),
            "error-handling-via-anyhow"
        );
    }

    #[test]
    fn test_slugify_long_text() {
        let long = "This is a very long description that should be truncated to fifty characters max";
        let slug = slugify(long);
        assert!(slug.len() <= 50);
        assert!(!slug.ends_with('-'));
    }

    #[test]
    fn test_slugify_collapses_hyphens() {
        assert_eq!(slugify("foo   bar---baz"), "foo-bar-baz");
    }

    #[test]
    fn test_category_dirname() {
        assert_eq!(category_dirname(&ArfCategory::Decision), "decisions");
        assert_eq!(category_dirname(&ArfCategory::Pattern), "patterns");
        assert_eq!(category_dirname(&ArfCategory::Bug), "bugs");
        assert_eq!(category_dirname(&ArfCategory::Migration), "migrations");
        assert_eq!(category_dirname(&ArfCategory::Fact), "facts");
    }

    #[test]
    fn test_write_new_arf() -> Result<()> {
        let noggin_dir = setup_noggin_dir();
        let arf = ArfFile::new(
            "Use connection pooling pattern",
            "Reduces database overhead",
            "Configure PgBouncer with transaction mode",
        );

        let result = write_arfs(noggin_dir.path(), &[arf])?;

        assert_eq!(result.written, 1);
        assert_eq!(result.updated, 0);
        assert_eq!(result.skipped, 0);

        let written = noggin_dir
            .path()
            .join("patterns/use-connection-pooling-pattern.arf");
        assert!(written.exists());

        Ok(())
    }

    #[test]
    fn test_write_skips_identical() -> Result<()> {
        let noggin_dir = setup_noggin_dir();
        let arf = ArfFile::new(
            "Use connection pooling pattern",
            "Reduces database overhead",
            "Configure PgBouncer",
        );

        // Write once
        write_arfs(noggin_dir.path(), &[arf.clone()])?;

        // Write again - should skip
        let result = write_arfs(noggin_dir.path(), &[arf])?;
        assert_eq!(result.written, 0);
        assert_eq!(result.skipped, 1);

        Ok(())
    }

    #[test]
    fn test_write_updates_changed() -> Result<()> {
        let noggin_dir = setup_noggin_dir();
        let arf1 = ArfFile::new(
            "Use connection pooling pattern",
            "Reduces database overhead",
            "Configure PgBouncer v1",
        );

        write_arfs(noggin_dir.path(), &[arf1])?;

        let arf2 = ArfFile::new(
            "Use connection pooling pattern",
            "Reduces database overhead",
            "Configure PgBouncer v2 with improved settings",
        );

        let result = write_arfs(noggin_dir.path(), &[arf2])?;
        assert_eq!(result.updated, 1);
        assert_eq!(result.written, 0);

        Ok(())
    }

    #[test]
    fn test_write_categorizes_correctly() -> Result<()> {
        let noggin_dir = setup_noggin_dir();

        let decision = ArfFile::new("Decided to adopt Rust", "Performance", "Rewrote in Rust");
        let bug = ArfFile::new("Fixed memory leak bug", "Crash reports", "Added drop impl");
        let migration = ArfFile::new(
            "Database schema migration v2",
            "New features need columns",
            "ALTER TABLE",
        );

        write_arfs(noggin_dir.path(), &[decision, bug, migration])?;

        assert!(noggin_dir
            .path()
            .join("decisions/decided-to-adopt-rust.arf")
            .exists());
        assert!(noggin_dir
            .path()
            .join("bugs/fixed-memory-leak-bug.arf")
            .exists());
        assert!(noggin_dir
            .path()
            .join("migrations/database-schema-migration-v2.arf")
            .exists());

        Ok(())
    }
}
