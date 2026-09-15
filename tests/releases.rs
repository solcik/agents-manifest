use serde_json::json;
use tera::{Context, Tera};

fn render_changelog(contributors: serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let configuration: toml::Value = toml::from_str(include_str!("../release-plz.toml"))?;
    let template = configuration["changelog"]["body"]
        .as_str()
        .ok_or("Missing changelog body")?;
    let usernames = contributors
        .as_array()
        .ok_or("Expected contributor array")?;
    let mut commits: Vec<_> = usernames.iter().map(|contributor| json!({
        "group": "Added", "scope": "cli", "message": "implement skill manifests",
        "breaking": true,
        "links": [{"text": "#1", "href": "https://github.com/solcik/agents-manifest/pull/1"}],
        "remote": contributor
    })).collect();
    if commits.is_empty() {
        commits.push(json!({
            "group": "Added", "scope": null, "message": "unattributed change",
            "breaking": false, "links": [], "remote": {"username": null}
        }));
    }
    let context = Context::from_value(json!({
        "version": "0.1.0",
        "release_link": "https://github.com/solcik/agents-manifest/releases/tag/v0.1.0",
        "timestamp": 1757894400,
        "commits": commits,
        "remote": {"owner": "solcik", "repo": "agents-manifest"}
    }))?;
    Ok(Tera::one_off(template, &context, false)?)
}

#[test]
fn changelog_credits_contributors_and_preserves_change_links()
-> Result<(), Box<dyn std::error::Error>> {
    let rendered = render_changelog(json!([
        {"username": "solcik"},
        {"username": "solcik"},
        {"username": "davidsolc-ai"}
    ]))?;
    assert!(rendered.contains("### Contributors"));
    assert!(rendered.contains("- @davidsolc-ai"));
    assert!(rendered.contains("- @solcik"));
    assert_eq!(rendered.matches("- @solcik").count(), 1);
    assert!(rendered.find("@davidsolc-ai") < rendered.find("@solcik"));
    assert!(rendered.contains("**BREAKING:** **cli:** implement skill manifests"));
    assert!(rendered.contains("[#1](https://github.com/solcik/agents-manifest/pull/1)"));
    assert!(
        rendered.contains("[0.1.0](https://github.com/solcik/agents-manifest/releases/tag/v0.1.0)")
    );
    Ok(())
}

#[test]
fn changelog_omits_empty_contributor_sections() -> Result<(), Box<dyn std::error::Error>> {
    let rendered = render_changelog(json!([]))?;
    assert!(!rendered.contains("### Contributors"));
    assert!(!rendered.contains('@'));
    Ok(())
}

#[test]
fn changelog_requests_release_plz_commit_username_enrichment()
-> Result<(), Box<dyn std::error::Error>> {
    let configuration: toml::Value = toml::from_str(include_str!("../release-plz.toml"))?;
    let body = configuration["changelog"]["body"]
        .as_str()
        .ok_or("Missing changelog body")?;
    assert!(body.contains("remote.username"));
    assert!(!body.contains("remote.contributors"));
    Ok(())
}

#[test]
fn release_notes_reuse_the_changelog_without_duplicate_attribution()
-> Result<(), Box<dyn std::error::Error>> {
    let configuration: toml::Value = toml::from_str(include_str!("../release-plz.toml"))?;
    assert_eq!(
        configuration["workspace"]["git_release_body"].as_str(),
        Some("{{ changelog }}")
    );
    Ok(())
}
