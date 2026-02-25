use crate::arf::ArfFile;
use super::conflict::FieldConflict;
use std::collections::HashMap;

/// How a conflict was resolved
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// 2+ models agreed (weighted score >= 2.0)
    MajorityVote { winner: String, vote_score: f64 },
    /// All different; picked the highest-weight model's value
    HighestWeight { model: String, weight: f64 },
    /// Values were non-contradictory and merged together
    Merged,
    /// Irreconcilable; kept as separate ARF entries
    KeepAll,
}

/// Default model weights for voting
fn model_weight(model: &str) -> f64 {
    match model.to_lowercase().as_str() {
        "claude" => 1.2,
        "gemini" => 1.1,
        "codex" => 1.0,
        _ => 1.0,
    }
}

/// Resolve a single field conflict via weighted majority voting.
pub fn resolve_conflict(conflict: &FieldConflict) -> Resolution {
    if conflict.values.is_empty() {
        return Resolution::KeepAll;
    }

    // Normalize values for comparison (trim, lowercase) but keep original casing
    let mut vote_map: HashMap<String, (f64, String)> = HashMap::new();

    for (model, value) in &conflict.values {
        let normalized = value.trim().to_lowercase();
        let weight = model_weight(model);

        let entry = vote_map
            .entry(normalized)
            .or_insert_with(|| (0.0, value.clone()));
        entry.0 += weight;
    }

    // Find the winner
    let mut candidates: Vec<(String, f64, String)> = vote_map
        .into_iter()
        .map(|(norm, (score, original))| (norm, score, original))
        .collect();
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if let Some((_, score, winner)) = candidates.first() {
        if *score >= 2.0 {
            return Resolution::MajorityVote {
                winner: winner.clone(),
                vote_score: *score,
            };
        }
    }

    // All different: pick highest-weight model
    let mut best_model = String::new();
    let mut best_weight: f64 = 0.0;

    for (model, _value) in &conflict.values {
        let weight = model_weight(model);
        if weight > best_weight {
            best_weight = weight;
            best_model = model.clone();
        }
    }

    if candidates.len() > 1 {
        Resolution::HighestWeight {
            model: best_model,
            weight: best_weight,
        }
    } else {
        Resolution::Merged
    }
}

/// Resolve all conflicts and apply resolutions to the merged ARFs.
///
/// Returns (resolved_arfs, resolved_count, manual_count).
pub fn resolve_all(
    mut arfs: Vec<ArfFile>,
    conflicts: Vec<FieldConflict>,
) -> (Vec<ArfFile>, usize, usize) {
    let mut resolved_count = 0;
    let mut manual_count = 0;

    for conflict in &conflicts {
        let resolution = resolve_conflict(conflict);

        match &resolution {
            Resolution::MajorityVote { winner, .. } => {
                apply_resolution(&mut arfs, &conflict.field, winner);
                resolved_count += 1;
            }
            Resolution::HighestWeight { model, .. } => {
                // Find the value from the highest-weight model
                if let Some((_, value)) = conflict.values.iter().find(|(m, _)| m == model) {
                    apply_resolution(&mut arfs, &conflict.field, value);
                }
                resolved_count += 1;
            }
            Resolution::Merged => {
                resolved_count += 1;
            }
            Resolution::KeepAll => {
                manual_count += 1;
            }
        }
    }

    (arfs, resolved_count, manual_count)
}

/// Apply a resolved value to the appropriate field in the ARF list.
fn apply_resolution(arfs: &mut [ArfFile], field: &str, value: &str) {
    if arfs.is_empty() {
        return;
    }

    match field {
        "what" => {
            arfs[0].what = value.to_string();
        }
        "why" => {
            arfs[0].why = value.to_string();
        }
        "how" => {
            arfs[0].how = value.to_string();
        }
        f if f.starts_with("context.outcome.") => {
            let key = f.strip_prefix("context.outcome.").unwrap_or(f);
            arfs[0].context.outcome.insert(key.to_string(), value.to_string());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::conflict::ConflictKind;

    #[test]
    fn test_model_weights() {
        assert_eq!(model_weight("claude"), 1.2);
        assert_eq!(model_weight("gemini"), 1.1);
        assert_eq!(model_weight("codex"), 1.0);
        assert_eq!(model_weight("unknown"), 1.0);
    }

    #[test]
    fn test_resolve_majority_vote() {
        let conflict = FieldConflict {
            field: "what".to_string(),
            kind: ConflictKind::DifferentValues,
            values: vec![
                ("claude".to_string(), "Use pooling".to_string()),
                ("gemini".to_string(), "Use pooling".to_string()),
                ("codex".to_string(), "Use connection pooling".to_string()),
            ],
            resolution: None,
        };

        let resolution = resolve_conflict(&conflict);
        match resolution {
            Resolution::MajorityVote { winner, vote_score } => {
                assert_eq!(winner, "Use pooling");
                // claude 1.2 + gemini 1.1 = 2.3
                assert!((vote_score - 2.3).abs() < 0.01);
            }
            _ => panic!("Expected MajorityVote"),
        }
    }

    #[test]
    fn test_resolve_highest_weight() {
        let conflict = FieldConflict {
            field: "what".to_string(),
            kind: ConflictKind::DifferentValues,
            values: vec![
                ("claude".to_string(), "Option A".to_string()),
                ("gemini".to_string(), "Option B".to_string()),
                ("codex".to_string(), "Option C".to_string()),
            ],
            resolution: None,
        };

        let resolution = resolve_conflict(&conflict);
        match resolution {
            Resolution::HighestWeight { model, weight } => {
                assert_eq!(model, "claude");
                assert!((weight - 1.2).abs() < 0.01);
            }
            _ => panic!("Expected HighestWeight"),
        }
    }

    #[test]
    fn test_resolve_case_insensitive() {
        let conflict = FieldConflict {
            field: "what".to_string(),
            kind: ConflictKind::DifferentValues,
            values: vec![
                ("claude".to_string(), "Use Pooling".to_string()),
                ("gemini".to_string(), "use pooling".to_string()),
                ("codex".to_string(), "Something else".to_string()),
            ],
            resolution: None,
        };

        let resolution = resolve_conflict(&conflict);
        match resolution {
            Resolution::MajorityVote { vote_score, .. } => {
                // claude 1.2 + gemini 1.1 = 2.3 (case-insensitive match)
                assert!((vote_score - 2.3).abs() < 0.01);
            }
            _ => panic!("Expected MajorityVote"),
        }
    }

    #[test]
    fn test_resolve_empty_values() {
        let conflict = FieldConflict {
            field: "what".to_string(),
            kind: ConflictKind::DifferentValues,
            values: vec![],
            resolution: None,
        };

        assert_eq!(resolve_conflict(&conflict), Resolution::KeepAll);
    }

    #[test]
    fn test_resolve_all_applies_resolutions() {
        let arfs = vec![ArfFile::new("Original", "Reason", "Steps")];
        let conflicts = vec![FieldConflict {
            field: "what".to_string(),
            kind: ConflictKind::DifferentValues,
            values: vec![
                ("claude".to_string(), "Better name".to_string()),
                ("gemini".to_string(), "Better name".to_string()),
            ],
            resolution: None,
        }];

        let (resolved, count, manual) = resolve_all(arfs, conflicts);
        assert_eq!(resolved[0].what, "Better name");
        assert_eq!(count, 1);
        assert_eq!(manual, 0);
    }

    #[test]
    fn test_apply_resolution_outcome() {
        let mut arfs = vec![ArfFile::new("Test", "Why", "How")];
        apply_resolution(&mut arfs, "context.outcome.result", "success");
        assert_eq!(
            arfs[0].context.outcome.get("result"),
            Some(&"success".to_string())
        );
    }
}
