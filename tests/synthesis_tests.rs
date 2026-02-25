use llm_noggin::arf::ArfFile;
use llm_noggin::synthesis::{
    self, ModelOutput,
    merger, conflict, vote,
};

fn make_arf(what: &str, why: &str, how: &str) -> ArfFile {
    ArfFile::new(what, why, how)
}

fn make_output(model: &str, arfs: Vec<ArfFile>) -> ModelOutput {
    ModelOutput {
        model_name: model.to_string(),
        arf_files: arfs,
    }
}

// --- Full pipeline tests ---

#[test]
fn test_full_pipeline_three_models_agree() {
    let outputs = vec![
        make_output("claude", vec![
            make_arf("Use connection pooling", "Reduces overhead", "Configure PgBouncer"),
        ]),
        make_output("gemini", vec![
            make_arf("Use connection pooling", "Reduces overhead", "Configure PgBouncer"),
        ]),
        make_output("codex", vec![
            make_arf("Use connection pooling", "Reduces overhead", "Configure PgBouncer"),
        ]),
    ];

    let result = synthesis::synthesize(outputs).unwrap();
    assert_eq!(result.unified_arfs.len(), 1);
    assert_eq!(result.unified_arfs[0].what, "Use connection pooling");
    assert_eq!(result.report.models_used.len(), 3);
    assert_eq!(result.report.total_input_arfs, 3);
}

#[test]
fn test_full_pipeline_different_topics() {
    let outputs = vec![
        make_output("claude", vec![
            make_arf("Use pooling", "Performance", "PgBouncer"),
            make_arf("Add caching", "Speed", "Redis"),
        ]),
        make_output("gemini", vec![
            make_arf("Use pooling", "Lower latency", "PgBouncer setup"),
            make_arf("Add caching", "Throughput", "Redis cluster"),
        ]),
    ];

    let result = synthesis::synthesize(outputs).unwrap();
    // Should produce 2 unified ARFs (pooling + caching)
    assert_eq!(result.unified_arfs.len(), 2);
    assert_eq!(result.report.total_input_arfs, 4);
}

#[test]
fn test_full_pipeline_majority_wins_what_field() {
    // Values within edit distance < 3 so they cluster together
    let outputs = vec![
        make_output("claude", vec![
            make_arf("Use pooling", "Perf", "Setup"),
        ]),
        make_output("gemini", vec![
            make_arf("Use pooling", "Perf", "Setup"),
        ]),
        make_output("codex", vec![
            make_arf("Use poolings", "Perf", "Setup"),
        ]),
    ];

    let result = synthesis::synthesize(outputs).unwrap();
    assert_eq!(result.unified_arfs.len(), 1);
    // "Use pooling" has 2 votes (claude + gemini), should win
    assert_eq!(result.unified_arfs[0].what, "Use pooling");
}

#[test]
fn test_full_pipeline_merges_context() {
    let mut arf1 = make_arf("Fix bug", "Crashes", "Add check");
    arf1.add_file("src/main.rs");
    arf1.add_commit("abc123");

    let mut arf2 = make_arf("Fix bug", "Crashes", "Add check");
    arf2.add_file("src/lib.rs");
    arf2.add_commit("def456");

    let outputs = vec![
        make_output("claude", vec![arf1]),
        make_output("gemini", vec![arf2]),
    ];

    let result = synthesis::synthesize(outputs).unwrap();
    assert_eq!(result.unified_arfs.len(), 1);
    let ctx = &result.unified_arfs[0].context;
    // Files and commits should be unioned and sorted
    assert!(ctx.files.contains(&"src/lib.rs".to_string()));
    assert!(ctx.files.contains(&"src/main.rs".to_string()));
    assert!(ctx.commits.contains(&"abc123".to_string()));
    assert!(ctx.commits.contains(&"def456".to_string()));
}

#[test]
fn test_determinism_same_input_same_output() {
    let make_inputs = || {
        vec![
            make_output("claude", vec![
                make_arf("Use pooling", "Performance", "PgBouncer\nSet max connections"),
            ]),
            make_output("gemini", vec![
                make_arf("Use pooling", "Lower latency", "PgBouncer\nMonitor connections"),
            ]),
            make_output("codex", vec![
                make_arf("Use pooling", "Speed improvement", "PgBouncer"),
            ]),
        ]
    };

    let result1 = synthesis::synthesize(make_inputs()).unwrap();
    let result2 = synthesis::synthesize(make_inputs()).unwrap();

    assert_eq!(result1.unified_arfs.len(), result2.unified_arfs.len());
    for (a, b) in result1.unified_arfs.iter().zip(result2.unified_arfs.iter()) {
        assert_eq!(a.what, b.what);
        assert_eq!(a.why, b.why);
        assert_eq!(a.how, b.how);
        assert_eq!(a.context.files, b.context.files);
        assert_eq!(a.context.commits, b.context.commits);
    }
}

// --- Parser tests ---

#[test]
fn test_parse_model_response_single_block() {
    let raw = r#"
what = "Test"
why = "Reason"
how = "Steps"
"#;
    let arfs = synthesis::parse_model_response("claude", raw).unwrap();
    assert_eq!(arfs.len(), 1);
    assert_eq!(arfs[0].what, "Test");
}

#[test]
fn test_parse_model_response_with_context() {
    let raw = r#"
what = "Test"
why = "Reason"
how = "Steps"

[context]
files = ["src/main.rs"]
commits = ["abc123"]
"#;
    let arfs = synthesis::parse_model_response("claude", raw).unwrap();
    assert_eq!(arfs[0].context.files, vec!["src/main.rs"]);
}

// --- Voting tests ---

#[test]
fn test_voting_weighted_scores() {
    let conflict = conflict::FieldConflict {
        field: "what".to_string(),
        kind: conflict::ConflictKind::DifferentValues,
        values: vec![
            ("claude".to_string(), "A".to_string()),  // 1.2
            ("gemini".to_string(), "A".to_string()),   // 1.1
            ("codex".to_string(), "B".to_string()),    // 1.0
        ],
        resolution: None,
    };

    let resolution = vote::resolve_conflict(&conflict);
    match resolution {
        vote::Resolution::MajorityVote { winner, vote_score } => {
            assert_eq!(winner, "A");
            assert!((vote_score - 2.3).abs() < 0.01);
        }
        _ => panic!("Expected MajorityVote"),
    }
}

// --- Grouping tests ---

#[test]
fn test_similarity_clustering_edit_distance() {
    let tagged = vec![
        ("claude".to_string(), make_arf("Use pool", "A", "B")),
        ("gemini".to_string(), make_arf("Use pools", "C", "D")),  // distance 1
        ("codex".to_string(), make_arf("Add cache", "E", "F")),   // distance >> 3
    ];

    let clusters = merger::group_by_similarity(&tagged);
    assert_eq!(clusters.len(), 2);
    // First cluster: "Use pool" + "Use pools"
    assert_eq!(clusters[0].len(), 2);
    // Second cluster: "Add cache"
    assert_eq!(clusters[1].len(), 1);
}

#[test]
fn test_category_grouping() {
    let tagged = vec![
        ("claude".to_string(), make_arf("Fix null bug", "Crash", "Check nil")),
        ("gemini".to_string(), make_arf("Migrate to v3", "Upgrade", "Run script")),
        ("codex".to_string(), make_arf("API returns JSON", "Spec says so", "Parse response")),
    ];

    let groups = merger::group_by_category(&tagged);
    assert!(groups.contains_key(&merger::ArfCategory::Bug));
    assert!(groups.contains_key(&merger::ArfCategory::Migration));
    assert!(groups.contains_key(&merger::ArfCategory::Fact));
}

// --- Edge cases ---

#[test]
fn test_synthesize_single_model_single_arf() {
    let result = synthesis::synthesize(vec![
        make_output("claude", vec![make_arf("Only entry", "Only reason", "Only step")]),
    ]).unwrap();

    assert_eq!(result.unified_arfs.len(), 1);
    assert_eq!(result.report.conflicts_detected, 0);
}

#[test]
fn test_synthesize_empty_arfs_errors() {
    let result = synthesis::synthesize(vec![
        make_output("claude", vec![]),
        make_output("gemini", vec![]),
    ]);
    assert!(result.is_err());
}

#[test]
fn test_why_merges_unique_sentences() {
    let outputs = vec![
        make_output("claude", vec![
            make_arf("X", "Performance boost. Less overhead", "Y"),
        ]),
        make_output("gemini", vec![
            make_arf("X", "Performance boost. Better throughput", "Y"),
        ]),
    ];

    let result = synthesis::synthesize(outputs).unwrap();
    let why = &result.unified_arfs[0].why;
    assert!(why.contains("Performance boost"));
    assert!(why.contains("Less overhead"));
    assert!(why.contains("Better throughput"));
}
