use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// ARF (Augmented Reasoning Format) file structure
/// Stores codebase knowledge as structured TOML with what/why/how/context sections
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArfFile {
    /// What: Concise description of the knowledge
    pub what: String,
    
    /// Why: Reason or motivation behind this knowledge
    pub why: String,
    
    /// How: Implementation details or process
    pub how: String,
    
    /// Optional context with additional metadata
    #[serde(default)]
    pub context: ArfContext,
}

/// Context section with metadata about the knowledge
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ArfContext {
    /// Files related to this knowledge
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    
    /// Git commits related to this knowledge
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<String>,
    
    /// Dependencies required
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<String>,
    
    /// Outcome or result (key-value pairs)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub outcome: HashMap<String, String>,
}

impl ArfFile {
    /// Create a new ARF file with required fields
    pub fn new(what: impl Into<String>, why: impl Into<String>, how: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            why: why.into(),
            how: how.into(),
            context: ArfContext::default(),
        }
    }
    
    /// Load ARF file from TOML file
    pub fn from_toml(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read ARF file: {}", path.display()))?;
        
        let arf: ArfFile = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse TOML in: {}", path.display()))?;
        
        Ok(arf)
    }
    
    /// Write ARF file to TOML file
    pub fn to_toml(&self, path: &Path) -> Result<()> {
        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }
        
        let toml_string = toml::to_string_pretty(self)
            .context("Failed to serialize ARF file to TOML")?;
        
        fs::write(path, toml_string)
            .with_context(|| format!("Failed to write ARF file: {}", path.display()))?;
        
        Ok(())
    }
    
    /// Validate that required fields are present and non-empty
    pub fn validate(&self) -> Result<()> {
        if self.what.trim().is_empty() {
            anyhow::bail!("ARF file missing required field: what");
        }
        
        if self.why.trim().is_empty() {
            anyhow::bail!("ARF file missing required field: why");
        }
        
        if self.how.trim().is_empty() {
            anyhow::bail!("ARF file missing required field: how");
        }
        
        Ok(())
    }
    
    /// Add a file path to the context
    pub fn add_file(&mut self, path: impl Into<String>) {
        self.context.files.push(path.into());
    }
    
    /// Add a commit hash to the context
    pub fn add_commit(&mut self, commit: impl Into<String>) {
        self.context.commits.push(commit.into());
    }
    
    /// Add a dependency to the context
    pub fn add_dependency(&mut self, dep: impl Into<String>) {
        self.context.dependencies.push(dep.into());
    }
    
    /// Add an outcome key-value pair to the context
    pub fn add_outcome(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.context.outcome.insert(key.into(), value.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_new_arf_file() {
        let arf = ArfFile::new("Test decision", "Because reasons", "Implementation steps");
        
        assert_eq!(arf.what, "Test decision");
        assert_eq!(arf.why, "Because reasons");
        assert_eq!(arf.how, "Implementation steps");
        assert!(arf.context.files.is_empty());
        assert!(arf.context.commits.is_empty());
    }
    
    #[test]
    fn test_add_context_helpers() {
        let mut arf = ArfFile::new("Decision", "Reason", "Steps");
        
        arf.add_file("src/main.rs");
        arf.add_file("src/lib.rs");
        arf.add_commit("abc123");
        arf.add_dependency("serde");
        arf.add_outcome("result", "success");
        
        assert_eq!(arf.context.files, vec!["src/main.rs", "src/lib.rs"]);
        assert_eq!(arf.context.commits, vec!["abc123"]);
        assert_eq!(arf.context.dependencies, vec!["serde"]);
        assert_eq!(arf.context.outcome.get("result"), Some(&"success".to_string()));
    }
    
    #[test]
    fn test_validate_success() {
        let arf = ArfFile::new("What", "Why", "How");
        assert!(arf.validate().is_ok());
    }
    
    #[test]
    fn test_validate_missing_what() {
        let arf = ArfFile::new("", "Why", "How");
        let result = arf.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("what"));
    }
    
    #[test]
    fn test_validate_missing_why() {
        let arf = ArfFile::new("What", "", "How");
        let result = arf.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("why"));
    }
    
    #[test]
    fn test_validate_missing_how() {
        let arf = ArfFile::new("What", "Why", "");
        let result = arf.validate();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("how"));
    }
    
    #[test]
    fn test_round_trip_serialization() {
        let tmp_dir = TempDir::new().unwrap();
        let file_path = tmp_dir.path().join("test.arf");
        
        let mut original = ArfFile::new(
            "Adopt ActivityPub",
            "Wide adoption, reduces dev time",
            "Implement federation endpoints"
        );
        original.add_file("app/services/activitypub/");
        original.add_commit("a1b2c3d");
        original.add_dependency("httparty");
        original.add_outcome("result", "success");
        
        // Write to file
        original.to_toml(&file_path).unwrap();
        
        // Read back
        let loaded = ArfFile::from_toml(&file_path).unwrap();
        
        // Should be identical
        assert_eq!(original, loaded);
    }
    
    #[test]
    fn test_from_toml_missing_file() {
        let result = ArfFile::from_toml(Path::new("/nonexistent/file.arf"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to read"));
    }
    
    #[test]
    fn test_from_toml_malformed() {
        let tmp_dir = TempDir::new().unwrap();
        let file_path = tmp_dir.path().join("bad.arf");
        
        fs::write(&file_path, "this is not valid toml {[").unwrap();
        
        let result = ArfFile::from_toml(&file_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Failed to parse"));
    }
    
    #[test]
    fn test_to_toml_creates_directories() {
        let tmp_dir = TempDir::new().unwrap();
        let nested_path = tmp_dir.path().join("decisions").join("nested").join("test.arf");
        
        let arf = ArfFile::new("Test", "Reason", "Steps");
        
        // Should create parent directories
        arf.to_toml(&nested_path).unwrap();
        
        assert!(nested_path.exists());
    }
    
    #[test]
    fn test_context_default_empty() {
        let context = ArfContext::default();
        
        assert!(context.files.is_empty());
        assert!(context.commits.is_empty());
        assert!(context.dependencies.is_empty());
        assert!(context.outcome.is_empty());
    }
}
