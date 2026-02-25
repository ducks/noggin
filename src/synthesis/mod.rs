pub mod conflict;
pub mod merger;
pub mod vote;

use crate::arf::ArfFile;
use crate::error::{Error, SynthesisError};

/// Output from a single model's analysis
#[derive(Debug, Clone)]
pub struct ModelOutput {
    pub model_name: String,
    pub arf_files: Vec<ArfFile>,
}

/// Result of the synthesis pipeline
#[derive(Debug, Clone)]
pub struct SynthesisResult {
    pub unified_arfs: Vec<ArfFile>,
    pub report: SynthesisReport,
}

/// Statistics about the synthesis process
#[derive(Debug, Clone)]
pub struct SynthesisReport {
    pub total_input_arfs: usize,
    pub total_output_arfs: usize,
    pub conflicts_detected: usize,
    pub conflicts_resolved: usize,
    pub conflicts_manual: usize,
    pub model_agreement_pct: f64,
    pub models_used: Vec<String>,
}

/// Parse a model's raw text response into a list of ARF files.
///
/// Tries TOML array-of-tables first (multiple `[[entry]]` blocks),
/// then falls back to splitting on `---` delimiters and parsing
/// each section as standalone TOML.
pub fn parse_model_response(model_name: &str, raw: &str) -> Result<Vec<ArfFile>, Error> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::Synthesis(SynthesisError::ParseFailed {
            model: model_name.to_string(),
            details: "empty response".to_string(),
        }));
    }

    // Strategy 1: Try parsing as a TOML document with [[entry]] array
    if let Ok(arfs) = parse_toml_array(trimmed) {
        if !arfs.is_empty() {
            return Ok(arfs);
        }
    }

    // Strategy 2: Split on --- delimiters and parse each block
    let blocks: Vec<&str> = trimmed
        .split("\n---\n")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    // If no --- delimiters, try the whole thing as a single TOML doc
    if blocks.len() <= 1 {
        if let Ok(arf) = parse_single_toml(trimmed) {
            return Ok(vec![arf]);
        }
    }

    let mut arfs = Vec::new();
    for block in &blocks {
        if let Ok(arf) = parse_single_toml(block) {
            arfs.push(arf);
        }
    }

    if arfs.is_empty() {
        return Err(Error::Synthesis(SynthesisError::ParseFailed {
            model: model_name.to_string(),
            details: format!("no valid TOML blocks found in {} chars of output", trimmed.len()),
        }));
    }

    Ok(arfs)
}

/// Try to parse TOML with `[[entry]]` array-of-tables syntax
fn parse_toml_array(raw: &str) -> Result<Vec<ArfFile>, ()> {
    #[derive(serde::Deserialize)]
    struct Wrapper {
        #[serde(default)]
        entry: Vec<ArfFile>,
    }

    let wrapper: Wrapper = toml::from_str(raw).map_err(|_| ())?;
    Ok(wrapper.entry)
}

/// Parse a single TOML block as an ArfFile
fn parse_single_toml(raw: &str) -> Result<ArfFile, ()> {
    toml::from_str::<ArfFile>(raw).map_err(|_| ())
}

/// Run the full synthesis pipeline on outputs from multiple models.
///
/// 1. Parse raw responses into ArfFiles
/// 2. Group by category and similarity
/// 3. Merge clusters
/// 4. Detect and resolve conflicts
/// 5. Normalize and return
pub fn synthesize(outputs: Vec<ModelOutput>) -> Result<SynthesisResult, Error> {
    let models_used: Vec<String> = outputs.iter().map(|o| o.model_name.clone()).collect();
    let total_input_arfs: usize = outputs.iter().map(|o| o.arf_files.len()).sum();

    if total_input_arfs == 0 {
        return Err(Error::Synthesis(SynthesisError::NoValidEntries));
    }

    // Tag each ARF with its source model
    let mut tagged: Vec<(String, ArfFile)> = Vec::new();
    for output in &outputs {
        for arf in &output.arf_files {
            tagged.push((output.model_name.clone(), arf.clone()));
        }
    }

    // Group by inferred category
    let categories = merger::group_by_category(&tagged);

    // Within each category, cluster by similarity then merge
    let mut merged_arfs: Vec<ArfFile> = Vec::new();
    let mut all_conflicts: Vec<conflict::FieldConflict> = Vec::new();

    for (_category, group) in &categories {
        let clusters = merger::group_by_similarity(group);
        for cluster in &clusters {
            let (arf, conflicts) = merger::merge_arf_fields(cluster);
            all_conflicts.extend(conflicts);
            merged_arfs.push(arf);
        }
    }

    // Detect any remaining conflicts
    let detected = conflict::detect_conflicts(&all_conflicts);
    let conflicts_detected = detected.len();

    // Resolve via voting
    let (resolved_arfs, resolved_count, manual_count) =
        vote::resolve_all(merged_arfs, detected);

    // Normalize: sort fields within each ARF, then sort ARFs
    let mut final_arfs = normalize_arfs(resolved_arfs);

    // Sort by category (inferred from context) then by `what`
    final_arfs.sort_by(|a, b| a.what.cmp(&b.what));

    let total_agreements = if total_input_arfs > 0 {
        let agreement_count = final_arfs.len() as f64;
        let input_count = total_input_arfs as f64;
        ((agreement_count / input_count) * 100.0).min(100.0)
    } else {
        0.0
    };

    let report = SynthesisReport {
        total_input_arfs,
        total_output_arfs: final_arfs.len(),
        conflicts_detected,
        conflicts_resolved: resolved_count,
        conflicts_manual: manual_count,
        model_agreement_pct: total_agreements,
        models_used,
    };

    Ok(SynthesisResult {
        unified_arfs: final_arfs,
        report,
    })
}

/// Normalize ARF files: sort Vec fields, trim whitespace
fn normalize_arfs(arfs: Vec<ArfFile>) -> Vec<ArfFile> {
    arfs.into_iter()
        .map(|mut arf| {
            arf.what = arf.what.trim().to_string();
            arf.why = arf.why.trim().to_string();
            arf.how = arf.how.trim().to_string();
            arf.context.files.sort();
            arf.context.files.dedup();
            arf.context.commits.sort();
            arf.context.commits.dedup();
            arf.context.dependencies.sort();
            arf.context.dependencies.dedup();
            arf
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_toml_block() {
        let raw = r#"
what = "Use connection pooling"
why = "Reduces database connection overhead"
how = "Configure PgBouncer with transaction mode"
"#;
        let arfs = parse_model_response("claude", raw).unwrap();
        assert_eq!(arfs.len(), 1);
        assert_eq!(arfs[0].what, "Use connection pooling");
    }

    #[test]
    fn test_parse_toml_array() {
        let raw = r#"
[[entry]]
what = "Use connection pooling"
why = "Performance"
how = "PgBouncer"

[[entry]]
what = "Add caching layer"
why = "Speed"
how = "Redis"
"#;
        let arfs = parse_model_response("claude", raw).unwrap();
        assert_eq!(arfs.len(), 2);
    }

    #[test]
    fn test_parse_dash_separated() {
        let raw = r#"what = "First entry"
why = "Reason one"
how = "Step one"
---
what = "Second entry"
why = "Reason two"
how = "Step two"
"#;
        let arfs = parse_model_response("gemini", raw).unwrap();
        assert_eq!(arfs.len(), 2);
        assert_eq!(arfs[0].what, "First entry");
        assert_eq!(arfs[1].what, "Second entry");
    }

    #[test]
    fn test_parse_empty_response() {
        let result = parse_model_response("codex", "");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_invalid_toml() {
        let result = parse_model_response("codex", "not valid toml at all {{{");
        assert!(result.is_err());
    }

    #[test]
    fn test_synthesize_empty_input() {
        let result = synthesize(vec![ModelOutput {
            model_name: "claude".to_string(),
            arf_files: vec![],
        }]);
        assert!(result.is_err());
    }

    #[test]
    fn test_synthesize_single_model() {
        let arf = ArfFile::new("Use pooling", "Performance", "PgBouncer");
        let result = synthesize(vec![ModelOutput {
            model_name: "claude".to_string(),
            arf_files: vec![arf],
        }])
        .unwrap();

        assert_eq!(result.unified_arfs.len(), 1);
        assert_eq!(result.report.total_input_arfs, 1);
        assert_eq!(result.report.models_used, vec!["claude"]);
    }

    #[test]
    fn test_normalize_trims_and_sorts() {
        let mut arf = ArfFile::new("  Test  ", " Why ", " How ");
        arf.context.files = vec!["b.rs".to_string(), "a.rs".to_string(), "a.rs".to_string()];
        arf.context.commits = vec!["def".to_string(), "abc".to_string()];

        let normalized = normalize_arfs(vec![arf]);
        assert_eq!(normalized[0].what, "Test");
        assert_eq!(normalized[0].context.files, vec!["a.rs", "b.rs"]);
        assert_eq!(normalized[0].context.commits, vec!["abc", "def"]);
    }
}
