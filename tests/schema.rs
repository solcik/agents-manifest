#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;

fn validator() -> jsonschema::Validator {
    let schema: Value =
        serde_json::from_str(include_str!("../schemas/agent-skills-manifest.schema.json")).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn example() -> Value {
    serde_yaml::from_str(include_str!("../examples/skills.yaml")).unwrap()
}

#[test]
fn schema_accepts_the_documented_manifest() {
    let validator = validator();
    let value = example();
    assert!(
        validator.is_valid(&value),
        "{:?}",
        validator.iter_errors(&value).collect::<Vec<_>>()
    );
}

#[test]
fn typed_manifest_roundtrip_matches_the_schema() {
    let manifest: agents_manifest::manifest::Manifest =
        serde_yaml::from_str(include_str!("../examples/skills.yaml")).unwrap();
    assert!(validator().is_valid(&serde_json::to_value(manifest).unwrap()));
}

#[test]
fn schema_rejects_unknown_fields_unpinned_revisions_and_unsafe_paths() {
    let validator = validator();
    for path in ["/skill", "../skill", "a/../skill", "a//b", "a/./b", "a\\b"] {
        let mut value = example();
        value["skills"][0]["path"] = path.into();
        assert!(!validator.is_valid(&value), "accepted path: {path}");
    }
    let mut value = example();
    value["extra"] = true.into();
    assert!(!validator.is_valid(&value));
    let mut value = example();
    value["skills"][0]["revision"] = "main".into();
    assert!(!validator.is_valid(&value));
    let mut value = example();
    value["skills"][0]["activation"] = "always".into();
    assert!(!validator.is_valid(&value));
}
