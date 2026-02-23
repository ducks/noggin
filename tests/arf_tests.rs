use llm_noggin::ArfFile;
use std::path::PathBuf;

/// Get path to test fixtures directory
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

#[test]
fn test_load_decision_fixture() {
    let path = fixtures_dir().join("decision.arf");
    let arf = ArfFile::from_toml(&path).expect("Failed to load decision.arf");
    
    assert_eq!(arf.what, "Adopt ActivityPub for federation");
    assert!(arf.why.contains("Wide adoption"));
    assert!(arf.how.contains("WebFinger"));
    assert!(arf.context.files.contains(&"app/services/activitypub/".to_string()));
    assert!(arf.context.commits.contains(&"a1b2c3d".to_string()));
    assert_eq!(arf.context.outcome.get("result"), Some(&"success".to_string()));
}

#[test]
fn test_load_pattern_fixture() {
    let path = fixtures_dir().join("pattern.arf");
    let arf = ArfFile::from_toml(&path).expect("Failed to load pattern.arf");
    
    assert_eq!(arf.what, "Error handling pattern in controllers");
    assert!(arf.context.files.contains(&"app/controllers/application_controller.rb".to_string()));
    assert_eq!(arf.context.outcome.get("pattern_type"), Some(&"error-handling".to_string()));
}

#[test]
fn test_load_migration_fixture() {
    let path = fixtures_dir().join("migration.arf");
    let arf = ArfFile::from_toml(&path).expect("Failed to load migration.arf");
    
    assert_eq!(arf.what, "Rails 7 to Rails 8 upgrade");
    assert!(arf.why.contains("Security patches"));
    assert_eq!(arf.context.commits.len(), 3);
    assert!(arf.context.dependencies.contains(&"rails".to_string()));
    assert_eq!(arf.context.outcome.get("duration"), Some(&"3 weeks".to_string()));
}

#[test]
fn test_all_fixtures_validate() {
    for fixture in ["decision.arf", "pattern.arf", "migration.arf"] {
        let path = fixtures_dir().join(fixture);
        let arf = ArfFile::from_toml(&path)
            .unwrap_or_else(|e| panic!("Failed to load {}: {}", fixture, e));
        
        arf.validate()
            .unwrap_or_else(|e| panic!("Validation failed for {}: {}", fixture, e));
    }
}
