use crate::arf::{ArfContext, ArfFile};
use super::conflict::FieldConflict;
use std::collections::HashMap;

/// Inferred ARF category for grouping
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ArfCategory {
    Decision,
    Pattern,
    Bug,
    Migration,
    Fact,
}

/// Group tagged ARFs by inferred category based on content heuristics.
pub fn group_by_category(
    tagged: &[(String, ArfFile)],
) -> HashMap<ArfCategory, Vec<(String, ArfFile)>> {
    let mut groups: HashMap<ArfCategory, Vec<(String, ArfFile)>> = HashMap::new();

    for (model, arf) in tagged {
        let category = infer_category(arf);
        groups
            .entry(category)
            .or_default()
            .push((model.clone(), arf.clone()));
    }

    groups
}

/// Infer category from ARF content keywords
fn infer_category(arf: &ArfFile) -> ArfCategory {
    let combined = format!(
        "{} {} {}",
        arf.what.to_lowercase(),
        arf.why.to_lowercase(),
        arf.how.to_lowercase()
    );

    if combined.contains("migrat") || combined.contains("upgrade") || combined.contains("schema") {
        ArfCategory::Migration
    } else if combined.contains("bug") || combined.contains("fix") || combined.contains("patch") {
        ArfCategory::Bug
    } else if combined.contains("pattern") || combined.contains("convention")
        || combined.contains("standard")
    {
        ArfCategory::Pattern
    } else if combined.contains("decid") || combined.contains("chose")
        || combined.contains("adopt") || combined.contains("decision")
    {
        ArfCategory::Decision
    } else {
        ArfCategory::Fact
    }
}

/// Within a category group, cluster ARFs by similarity of the `what` field.
/// Uses Levenshtein edit distance < 3 to decide if two ARFs describe the
/// same concept.
pub fn group_by_similarity(
    tagged: &[(String, ArfFile)],
) -> Vec<Vec<(String, ArfFile)>> {
    let mut clusters: Vec<Vec<(String, ArfFile)>> = Vec::new();

    for item in tagged {
        let what_lower = item.1.what.to_lowercase();
        let mut found = false;

        for cluster in &mut clusters {
            let representative = cluster[0].1.what.to_lowercase();
            let distance = edit_distance::edit_distance(&what_lower, &representative);
            if distance < 3 {
                cluster.push(item.clone());
                found = true;
                break;
            }
        }

        if !found {
            clusters.push(vec![item.clone()]);
        }
    }

    clusters
}

/// Merge a cluster of similar ARFs into a single unified ARF.
/// Returns the merged ARF and any field conflicts detected during merge.
pub fn merge_arf_fields(
    cluster: &[(String, ArfFile)],
) -> (ArfFile, Vec<FieldConflict>) {
    if cluster.len() == 1 {
        return (cluster[0].1.clone(), vec![]);
    }

    let mut conflicts = Vec::new();

    let what = merge_what(cluster, &mut conflicts);
    let why = merge_why(cluster);
    let how = merge_how(cluster);
    let context = merge_context(cluster, &mut conflicts);

    let arf = ArfFile {
        what,
        why,
        how,
        context,
    };

    (arf, conflicts)
}

/// Merge `what` fields: prefer shortest version appearing 2+ times,
/// else shortest overall.
fn merge_what(cluster: &[(String, ArfFile)], conflicts: &mut Vec<FieldConflict>) -> String {
    let mut counts: HashMap<String, Vec<String>> = HashMap::new();
    for (model, arf) in cluster {
        let normalized = arf.what.trim().to_string();
        counts
            .entry(normalized.clone())
            .or_default()
            .push(model.clone());
    }

    if counts.len() > 1 {
        let values: Vec<(String, String)> = cluster
            .iter()
            .map(|(m, a)| (m.clone(), a.what.trim().to_string()))
            .collect();

        conflicts.push(FieldConflict {
            field: "what".to_string(),
            kind: super::conflict::ConflictKind::DifferentValues,
            values,
            resolution: None,
        });
    }

    // Prefer shortest appearing 2+ times
    let mut majority: Vec<(&String, &Vec<String>)> = counts
        .iter()
        .filter(|(_, models)| models.len() >= 2)
        .collect();
    majority.sort_by_key(|(val, _)| val.len());

    if let Some((val, _)) = majority.first() {
        return val.to_string();
    }

    // Fall back to shortest overall
    let mut all: Vec<&String> = counts.keys().collect();
    all.sort_by_key(|v| v.len());
    all.first().map(|v| v.to_string()).unwrap_or_default()
}

/// Merge `why` fields: split on sentence boundaries, collect unique sentences.
fn merge_why(cluster: &[(String, ArfFile)]) -> String {
    let mut seen = Vec::new();

    for (_, arf) in cluster {
        let sentences = split_sentences(&arf.why);
        for sentence in sentences {
            let trimmed = sentence.trim().to_string();
            if !trimmed.is_empty() && !seen.contains(&trimmed) {
                seen.push(trimmed);
            }
        }
    }

    seen.join(". ")
}

/// Merge `how` fields: split on newlines, collect unique steps preserving
/// majority order.
fn merge_how(cluster: &[(String, ArfFile)]) -> String {
    let mut all_steps: Vec<String> = Vec::new();

    for (_, arf) in cluster {
        let steps: Vec<String> = arf
            .how
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        for step in steps {
            if !all_steps.contains(&step) {
                all_steps.push(step);
            }
        }
    }

    all_steps.join("\n")
}

/// Merge context fields from all ARFs in a cluster.
fn merge_context(
    cluster: &[(String, ArfFile)],
    conflicts: &mut Vec<FieldConflict>,
) -> ArfContext {
    let mut files: Vec<String> = Vec::new();
    let mut commits: Vec<String> = Vec::new();
    let mut dependencies: Vec<String> = Vec::new();
    let mut outcomes: HashMap<String, Vec<(String, String)>> = HashMap::new();

    for (model, arf) in cluster {
        for f in &arf.context.files {
            if !files.contains(f) {
                files.push(f.clone());
            }
        }
        for c in &arf.context.commits {
            if !commits.contains(c) {
                commits.push(c.clone());
            }
        }
        for d in &arf.context.dependencies {
            if !dependencies.contains(d) {
                dependencies.push(d.clone());
            }
        }
        for (key, value) in &arf.context.outcome {
            outcomes
                .entry(key.clone())
                .or_default()
                .push((model.clone(), value.clone()));
        }
    }

    files.sort();
    commits.sort();
    dependencies.sort();

    // Merge outcomes, flagging conflicts
    let mut merged_outcome: HashMap<String, String> = HashMap::new();
    for (key, model_values) in &outcomes {
        let unique_values: Vec<&String> = {
            let mut vals: Vec<&String> = model_values.iter().map(|(_, v)| v).collect();
            vals.dedup();
            vals
        };

        if unique_values.len() == 1 {
            merged_outcome.insert(key.clone(), unique_values[0].clone());
        } else {
            // Conflict on outcome key
            let values: Vec<(String, String)> = model_values.clone();
            conflicts.push(FieldConflict {
                field: format!("context.outcome.{}", key),
                kind: super::conflict::ConflictKind::DifferentValues,
                values,
                resolution: None,
            });
            // Use first value as placeholder until voting resolves it
            merged_outcome.insert(key.clone(), model_values[0].1.clone());
        }
    }

    ArfContext {
        files,
        commits,
        dependencies,
        outcome: merged_outcome,
    }
}

/// Split text into sentences on period boundaries.
fn split_sentences(text: &str) -> Vec<String> {
    text.split('.')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_category_migration() {
        let arf = ArfFile::new("Database migration to v3", "Schema upgrade needed", "Run migrate");
        assert_eq!(infer_category(&arf), ArfCategory::Migration);
    }

    #[test]
    fn test_infer_category_bug() {
        let arf = ArfFile::new("Fix null pointer bug", "Crashes in prod", "Add nil check");
        assert_eq!(infer_category(&arf), ArfCategory::Bug);
    }

    #[test]
    fn test_infer_category_decision() {
        let arf = ArfFile::new("Adopt Redis for caching", "Decided after evaluation", "Install Redis");
        assert_eq!(infer_category(&arf), ArfCategory::Decision);
    }

    #[test]
    fn test_infer_category_fact() {
        let arf = ArfFile::new("API rate limit is 1000/hour", "Documented in spec", "Check headers");
        assert_eq!(infer_category(&arf), ArfCategory::Fact);
    }

    #[test]
    fn test_group_by_category() {
        let tagged = vec![
            ("claude".to_string(), ArfFile::new("Fix crash bug", "Prod issue", "Add check")),
            ("gemini".to_string(), ArfFile::new("Migrate database", "Upgrade needed", "Run script")),
        ];
        let groups = group_by_category(&tagged);
        assert!(groups.contains_key(&ArfCategory::Bug));
        assert!(groups.contains_key(&ArfCategory::Migration));
    }

    #[test]
    fn test_group_by_similarity_clusters_similar() {
        let tagged = vec![
            ("claude".to_string(), ArfFile::new("Use pooling", "Perf", "Setup")),
            ("gemini".to_string(), ArfFile::new("Use pooling", "Speed", "Config")),
            ("codex".to_string(), ArfFile::new("Add caching", "Fast", "Redis")),
        ];
        let clusters = group_by_similarity(&tagged);
        // "Use pooling" x2 should cluster, "Add caching" separate
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].len(), 2);
        assert_eq!(clusters[1].len(), 1);
    }

    #[test]
    fn test_group_by_similarity_all_different() {
        let tagged = vec![
            ("claude".to_string(), ArfFile::new("Use pooling", "A", "B")),
            ("gemini".to_string(), ArfFile::new("Add caching", "C", "D")),
            ("codex".to_string(), ArfFile::new("Fix logging", "E", "F")),
        ];
        let clusters = group_by_similarity(&tagged);
        assert_eq!(clusters.len(), 3);
    }

    #[test]
    fn test_merge_single_item_cluster() {
        let cluster = vec![
            ("claude".to_string(), ArfFile::new("Test", "Reason", "Step")),
        ];
        let (arf, conflicts) = merge_arf_fields(&cluster);
        assert_eq!(arf.what, "Test");
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_merge_what_majority_wins() {
        let cluster = vec![
            ("claude".to_string(), ArfFile::new("Use pooling", "A", "B")),
            ("gemini".to_string(), ArfFile::new("Use pooling", "C", "D")),
            ("codex".to_string(), ArfFile::new("Use connection pooling", "E", "F")),
        ];
        let (arf, _) = merge_arf_fields(&cluster);
        assert_eq!(arf.what, "Use pooling");
    }

    #[test]
    fn test_merge_why_collects_unique_sentences() {
        let cluster = vec![
            ("claude".to_string(), ArfFile::new("X", "Performance boost. Less overhead", "Y")),
            ("gemini".to_string(), ArfFile::new("X", "Performance boost. Better throughput", "Y")),
        ];
        let (arf, _) = merge_arf_fields(&cluster);
        assert!(arf.why.contains("Performance boost"));
        assert!(arf.why.contains("Less overhead"));
        assert!(arf.why.contains("Better throughput"));
    }

    #[test]
    fn test_merge_how_collects_unique_steps() {
        let cluster = vec![
            ("claude".to_string(), ArfFile::new("X", "Y", "Step 1\nStep 2")),
            ("gemini".to_string(), ArfFile::new("X", "Y", "Step 1\nStep 3")),
        ];
        let (arf, _) = merge_arf_fields(&cluster);
        let steps: Vec<&str> = arf.how.lines().collect();
        assert_eq!(steps.len(), 3);
        assert!(steps.contains(&"Step 1"));
        assert!(steps.contains(&"Step 2"));
        assert!(steps.contains(&"Step 3"));
    }

    #[test]
    fn test_merge_context_unions_files() {
        let mut arf1 = ArfFile::new("X", "Y", "Z");
        arf1.add_file("a.rs");
        arf1.add_file("b.rs");
        let mut arf2 = ArfFile::new("X", "Y", "Z");
        arf2.add_file("b.rs");
        arf2.add_file("c.rs");

        let cluster = vec![
            ("claude".to_string(), arf1),
            ("gemini".to_string(), arf2),
        ];
        let (arf, _) = merge_arf_fields(&cluster);
        assert_eq!(arf.context.files, vec!["a.rs", "b.rs", "c.rs"]);
    }
}
