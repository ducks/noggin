//! Error types for noggin
//!
//! Comprehensive error handling for all failure modes:
//! - Manifest operations (file tracking, hashing, TOML parsing)
//! - Git operations (repo access, commit parsing, history walking)
//! - LLM requests (API failures, rate limits, malformed responses)
//! - ARF file operations (parsing, validation, schema)
//! - File I/O (reading, writing, permissions)

use std::fmt;
use std::io;

/// Result type alias for noggin operations
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type for noggin
#[derive(Debug)]
pub enum Error {
    /// Manifest-related errors
    Manifest(ManifestError),
    /// Git operation errors
    Git(GitError),
    /// LLM API errors
    Llm(LlmError),
    /// ARF file errors
    Arf(ArfError),
    /// I/O errors
    Io(IoError),
    /// Synthesis errors (consensus merging)
    Synthesis(SynthesisError),
}

/// Manifest operation errors
#[derive(Debug)]
pub enum ManifestError {
    /// File path not found in manifest
    FileNotFound(String),
    /// File hash mismatch on rescan (file changed unexpectedly)
    InvalidHash { path: String, expected: String, actual: String },
    /// Manifest TOML file is corrupted or invalid
    CorruptedData(String),
    /// Required field missing from manifest.toml schema
    MissingRequiredField(String),
}

/// Git operation errors
#[derive(Debug)]
pub enum GitError {
    /// Directory is not a git repository
    RepositoryNotFound(String),
    /// Commit hash not found in repository
    CommitNotFound(String),
    /// Invalid branch or tag reference
    InvalidRef(String),
    /// Underlying git2 library error
    GitCommandFailed { operation: String, source: String },
}

/// LLM API errors
#[derive(Debug)]
pub enum LlmError {
    /// HTTP request failed (network timeout, connection refused)
    RequestFailed { model: String, source: String },
    /// API response malformed (invalid JSON, missing fields)
    InvalidResponse { model: String, details: String },
    /// Rate limit exceeded (429 response)
    RateLimitExceeded { model: String, retry_after: Option<u64> },
    /// API authentication failed (invalid key)
    AuthenticationFailed(String),
    /// Model unavailable (503, model offline)
    ModelUnavailable(String),
}

/// ARF file errors
#[derive(Debug)]
pub enum ArfError {
    /// Failed to parse ARF file as TOML
    ParseFailed { path: String, source: String },
    /// Required ARF section missing (what/why/how)
    MissingSection { path: String, section: String },
    /// ARF structure doesn't match expected schema
    InvalidStructure { path: String, details: String },
    /// ARF file path doesn't exist
    InvalidPath(String),
}

/// Synthesis (consensus merging) errors
#[derive(Debug)]
pub enum SynthesisError {
    /// Failed to parse model output into ARF entries
    ParseFailed { model: String, details: String },
    /// No valid ARF entries found across all model outputs
    NoValidEntries,
    /// Conflict could not be resolved by any strategy
    UnresolvableConflict { field: String, models: Vec<String> },
}

/// File I/O errors
#[derive(Debug)]
pub enum IoError {
    /// Failed to read file
    FileReadFailed { path: String, source: io::Error },
    /// Failed to write file
    FileWriteFailed { path: String, source: io::Error },
    /// Failed to create directory
    DirectoryCreateFailed { path: String, source: io::Error },
    /// Permission denied
    PermissionDenied { path: String, source: io::Error },
    /// Other I/O error
    Other(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Manifest(e) => write!(f, "Manifest error: {}", e),
            Error::Git(e) => write!(f, "Git error: {}", e),
            Error::Llm(e) => write!(f, "LLM error: {}", e),
            Error::Arf(e) => write!(f, "ARF error: {}", e),
            Error::Io(e) => write!(f, "I/O error: {}", e),
            Error::Synthesis(e) => write!(f, "Synthesis error: {}", e),
        }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManifestError::FileNotFound(path) => {
                write!(f, "File not found in manifest: {}", path)
            }
            ManifestError::InvalidHash { path, expected, actual } => {
                write!(
                    f,
                    "Hash mismatch for {}: expected {}, got {}",
                    path, expected, actual
                )
            }
            ManifestError::CorruptedData(details) => {
                write!(f, "Manifest data corrupted: {}", details)
            }
            ManifestError::MissingRequiredField(field) => {
                write!(f, "Missing required field in manifest: {}", field)
            }
        }
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitError::RepositoryNotFound(path) => {
                write!(f, "Not a git repository: {}", path)
            }
            GitError::CommitNotFound(hash) => {
                write!(f, "Commit not found: {}", hash)
            }
            GitError::InvalidRef(ref_name) => {
                write!(f, "Invalid git reference: {}", ref_name)
            }
            GitError::GitCommandFailed { operation, source } => {
                write!(f, "Git operation '{}' failed: {}", operation, source)
            }
        }
    }
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::RequestFailed { model, source } => {
                write!(f, "Request to {} failed: {}", model, source)
            }
            LlmError::InvalidResponse { model, details } => {
                write!(f, "Invalid response from {}: {}", model, details)
            }
            LlmError::RateLimitExceeded { model, retry_after } => {
                match retry_after {
                    Some(seconds) => write!(
                        f,
                        "Rate limit exceeded for {} (retry after {} seconds)",
                        model, seconds
                    ),
                    None => write!(f, "Rate limit exceeded for {}", model),
                }
            }
            LlmError::AuthenticationFailed(model) => {
                write!(f, "Authentication failed for {}", model)
            }
            LlmError::ModelUnavailable(model) => {
                write!(f, "Model unavailable: {}", model)
            }
        }
    }
}

impl fmt::Display for ArfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArfError::ParseFailed { path, source } => {
                write!(f, "Failed to parse ARF file {}: {}", path, source)
            }
            ArfError::MissingSection { path, section } => {
                write!(f, "Missing required section '{}' in {}", section, path)
            }
            ArfError::InvalidStructure { path, details } => {
                write!(f, "Invalid ARF structure in {}: {}", path, details)
            }
            ArfError::InvalidPath(path) => {
                write!(f, "ARF file not found: {}", path)
            }
        }
    }
}

impl fmt::Display for SynthesisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SynthesisError::ParseFailed { model, details } => {
                write!(f, "Failed to parse {} output: {}", model, details)
            }
            SynthesisError::NoValidEntries => {
                write!(f, "No valid ARF entries found in any model output")
            }
            SynthesisError::UnresolvableConflict { field, models } => {
                write!(
                    f,
                    "Unresolvable conflict on field '{}' between models: {}",
                    field,
                    models.join(", ")
                )
            }
        }
    }
}

impl fmt::Display for IoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IoError::FileReadFailed { path, source } => {
                write!(f, "Failed to read {}: {}", path, source)
            }
            IoError::FileWriteFailed { path, source } => {
                write!(f, "Failed to write {}: {}", path, source)
            }
            IoError::DirectoryCreateFailed { path, source } => {
                write!(f, "Failed to create directory {}: {}", path, source)
            }
            IoError::PermissionDenied { path, source } => {
                write!(f, "Permission denied: {}: {}", path, source)
            }
            IoError::Other(source) => write!(f, "{}", source),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(IoError::FileReadFailed { source, .. })
            | Error::Io(IoError::FileWriteFailed { source, .. })
            | Error::Io(IoError::DirectoryCreateFailed { source, .. })
            | Error::Io(IoError::PermissionDenied { source, .. })
            | Error::Io(IoError::Other(source)) => Some(source),
            _ => None,
        }
    }
}

impl std::error::Error for ManifestError {}
impl std::error::Error for GitError {}
impl std::error::Error for LlmError {}
impl std::error::Error for ArfError {}
impl std::error::Error for SynthesisError {}
impl std::error::Error for IoError {}

// Conversion from std::io::Error
impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(IoError::Other(err))
    }
}

impl Error {
    /// Check if error is retryable (network issues, rate limits)
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Error::Llm(LlmError::RequestFailed { .. })
                | Error::Llm(LlmError::RateLimitExceeded { .. })
                | Error::Llm(LlmError::ModelUnavailable(_))
        )
    }

    /// Check if error is fatal (corrupted data, missing repo)
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            Error::Manifest(ManifestError::CorruptedData(_))
                | Error::Git(GitError::RepositoryNotFound(_))
                | Error::Llm(LlmError::AuthenticationFailed(_))
        )
    }

    /// Get formatted context string for logging
    pub fn context(&self) -> String {
        match self {
            Error::Manifest(e) => format!("manifest: {}", e),
            Error::Git(e) => format!("git: {}", e),
            Error::Llm(e) => format!("llm: {}", e),
            Error::Arf(e) => format!("arf: {}", e),
            Error::Io(e) => format!("io: {}", e),
            Error::Synthesis(e) => format!("synthesis: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error as StdError;

    #[test]
    fn test_manifest_error_display() {
        let err = Error::Manifest(ManifestError::FileNotFound("src/main.rs".to_string()));
        assert_eq!(
            err.to_string(),
            "Manifest error: File not found in manifest: src/main.rs"
        );
    }

    #[test]
    fn test_git_error_display() {
        let err = Error::Git(GitError::RepositoryNotFound("/tmp/notgit".to_string()));
        assert_eq!(
            err.to_string(),
            "Git error: Not a git repository: /tmp/notgit"
        );
    }

    #[test]
    fn test_llm_error_display() {
        let err = Error::Llm(LlmError::RateLimitExceeded {
            model: "gpt-4".to_string(),
            retry_after: Some(60),
        });
        assert_eq!(
            err.to_string(),
            "LLM error: Rate limit exceeded for gpt-4 (retry after 60 seconds)"
        );
    }

    #[test]
    fn test_arf_error_display() {
        let err = Error::Arf(ArfError::MissingSection {
            path: "decisions/auth.arf".to_string(),
            section: "what".to_string(),
        });
        assert_eq!(
            err.to_string(),
            "ARF error: Missing required section 'what' in decisions/auth.arf"
        );
    }

    #[test]
    fn test_io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err = Error::from(io_err);
        assert!(matches!(err, Error::Io(IoError::Other(_))));
    }

    #[test]
    fn test_is_retryable() {
        let retryable = Error::Llm(LlmError::RateLimitExceeded {
            model: "claude".to_string(),
            retry_after: None,
        });
        assert!(retryable.is_retryable());

        let not_retryable = Error::Manifest(ManifestError::CorruptedData(
            "invalid TOML".to_string(),
        ));
        assert!(!not_retryable.is_retryable());
    }

    #[test]
    fn test_is_fatal() {
        let fatal = Error::Git(GitError::RepositoryNotFound("/tmp".to_string()));
        assert!(fatal.is_fatal());

        let not_fatal = Error::Llm(LlmError::RequestFailed {
            model: "gemini".to_string(),
            source: "timeout".to_string(),
        });
        assert!(!not_fatal.is_fatal());
    }

    #[test]
    fn test_context() {
        let err = Error::Manifest(ManifestError::FileNotFound("test.rs".to_string()));
        assert_eq!(
            err.context(),
            "manifest: File not found in manifest: test.rs"
        );
    }

    #[test]
    fn test_error_source_chain() {
        let io_err = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let err = Error::Io(IoError::Other(io_err));
        assert!(err.source().is_some());
    }
}
