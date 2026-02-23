use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

const NOGGIN_DIR: &str = ".noggin";
const SUBDIRS: &[&str] = &["decisions", "migrations", "bugs", "patterns", "facts"];
const MANIFEST_TEMPLATE: &str = r#"# Noggin manifest - tracks analyzed files and commits
# This file is automatically managed by noggin

[files]
# Format: "path/to/file" = { hash = "sha256...", scanned = "YYYY-MM-DD", patterns = ["pattern-name"] }

[commits]
# Format: "commit-hash" = { processed = "YYYY-MM-DD", category = "decision|migration|bug", arf = "path/to/file.arf" }
"#;

pub fn init_command() -> Result<()> {
    let noggin_path = Path::new(NOGGIN_DIR);

    if noggin_path.exists() {
        anyhow::bail!(
            ".noggin/ directory already exists. Remove it first if you want to reinitialize."
        );
    }

    fs::create_dir(noggin_path)
        .context("Failed to create .noggin/ directory")?;

    println!("Created .noggin/ directory");

    for subdir in SUBDIRS {
        let subdir_path = noggin_path.join(subdir);
        fs::create_dir(&subdir_path)
            .with_context(|| format!("Failed to create {} directory", subdir))?;
        println!("  Created .noggin/{}/", subdir);
    }

    let manifest_path = noggin_path.join("manifest.toml");
    fs::write(&manifest_path, MANIFEST_TEMPLATE)
        .context("Failed to create manifest.toml")?;
    println!("  Created .noggin/manifest.toml");

    let gitignore_path = Path::new(".gitignore");
    if gitignore_path.exists() {
        let gitignore_content = fs::read_to_string(gitignore_path)
            .context("Failed to read .gitignore")?;
        
        if !gitignore_content.lines().any(|line| line.trim() == ".noggin/") {
            let mut new_content = gitignore_content;
            if !new_content.ends_with('\n') {
                new_content.push('\n');
            }
            new_content.push_str(".noggin/\n");
            
            fs::write(gitignore_path, new_content)
                .context("Failed to update .gitignore")?;
            println!("  Added .noggin/ to .gitignore");
        }
    } else {
        fs::write(gitignore_path, ".noggin/\n")
            .context("Failed to create .gitignore")?;
        println!("  Created .gitignore with .noggin/ entry");
    }

    println!("\n✓ Noggin initialized successfully!");
    println!("Run 'noggin learn' to start analyzing your codebase.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_init_creates_directory_structure() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        
        std::env::set_current_dir(temp_dir.path()).unwrap();

        let result = init_command();
        if let Err(e) = &result {
            eprintln!("init_command failed: {}", e);
        }
        assert!(result.is_ok());

        let noggin_path = temp_dir.path().join(".noggin");
        assert!(noggin_path.exists());
        assert!(noggin_path.is_dir());

        for subdir in SUBDIRS {
            let subdir_path = noggin_path.join(subdir);
            assert!(subdir_path.exists(), "Missing directory: {}", subdir);
            assert!(subdir_path.is_dir());
        }

        let manifest_path = noggin_path.join("manifest.toml");
        assert!(manifest_path.exists());
        let manifest_content = fs::read_to_string(&manifest_path).unwrap();
        assert!(manifest_content.contains("[files]"));
        assert!(manifest_content.contains("[commits]"));

        let gitignore_path = temp_dir.path().join(".gitignore");
        assert!(gitignore_path.exists());
        let gitignore_content = fs::read_to_string(&gitignore_path).unwrap();
        assert!(gitignore_content.contains(".noggin/"));

        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_init_fails_if_noggin_exists() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        
        std::env::set_current_dir(temp_dir.path()).unwrap();

        fs::create_dir(".noggin").unwrap();

        let result = init_command();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));

        std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn test_init_updates_existing_gitignore() {
        let temp_dir = TempDir::new().unwrap();
        let original_dir = std::env::current_dir().unwrap();
        
        std::env::set_current_dir(temp_dir.path()).unwrap();

        fs::write(".gitignore", "*.log\ntarget/\n").unwrap();

        init_command().unwrap();

        let gitignore_content = fs::read_to_string(".gitignore").unwrap();
        assert!(gitignore_content.contains("*.log"));
        assert!(gitignore_content.contains("target/"));
        assert!(gitignore_content.contains(".noggin/"));

        std::env::set_current_dir(original_dir).unwrap();
    }
}
