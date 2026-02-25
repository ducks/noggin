/// The kind of conflict between model outputs
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictKind {
    /// Models produced different values for the same field
    DifferentValues,
    /// Models produced structurally different outputs (e.g. list vs scalar)
    DifferentStructure,
    /// Field present in some model outputs but missing in others
    MissingInSome,
}

/// A conflict detected on a specific field during merging
#[derive(Debug, Clone)]
pub struct FieldConflict {
    /// Which field has the conflict (e.g. "what", "context.outcome.result")
    pub field: String,
    /// What kind of conflict
    pub kind: ConflictKind,
    /// The values each model produced: (model_name, value)
    pub values: Vec<(String, String)>,
    /// Resolution, if one has been applied
    pub resolution: Option<super::vote::Resolution>,
}

/// Filter conflicts that still need resolution (no resolution set yet).
pub fn detect_conflicts(conflicts: &[FieldConflict]) -> Vec<FieldConflict> {
    conflicts
        .iter()
        .filter(|c| c.resolution.is_none())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_conflicts_filters_unresolved() {
        let conflicts = vec![
            FieldConflict {
                field: "what".to_string(),
                kind: ConflictKind::DifferentValues,
                values: vec![
                    ("claude".to_string(), "A".to_string()),
                    ("gemini".to_string(), "B".to_string()),
                ],
                resolution: None,
            },
            FieldConflict {
                field: "why".to_string(),
                kind: ConflictKind::DifferentValues,
                values: vec![
                    ("claude".to_string(), "X".to_string()),
                    ("gemini".to_string(), "Y".to_string()),
                ],
                resolution: Some(super::super::vote::Resolution::Merged),
            },
        ];

        let unresolved = detect_conflicts(&conflicts);
        assert_eq!(unresolved.len(), 1);
        assert_eq!(unresolved[0].field, "what");
    }

    #[test]
    fn test_detect_conflicts_empty() {
        let unresolved = detect_conflicts(&[]);
        assert!(unresolved.is_empty());
    }

    #[test]
    fn test_conflict_kind_variants() {
        assert_eq!(ConflictKind::DifferentValues, ConflictKind::DifferentValues);
        assert_ne!(ConflictKind::DifferentValues, ConflictKind::MissingInSome);
    }
}
