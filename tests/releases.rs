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
fn commit_and_pr_title_checks_accept_the_same_types() -> Result<(), Box<dyn std::error::Error>> {
    let policy: toml::Value = toml::from_str(include_str!("../committed.toml"))?;
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(include_str!("../.github/workflows/pr-title.yaml"))?;
    let commit_types = policy["allowed_types"]
        .as_array()
        .ok_or("Missing commit types")?
        .iter()
        .map(|value| value.as_str().ok_or("Invalid commit type"))
        .collect::<Result<Vec<_>, _>>()?;
    let title_types = workflow["jobs"]["title"]["steps"][0]["with"]["types"]
        .as_str()
        .ok_or("Missing PR title types")?
        .split_whitespace()
        .collect::<Vec<_>>();
    assert_eq!(commit_types, title_types);
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

#[test]
fn pr_title_check_runs_on_the_events_release_pull_requests_produce()
-> Result<(), Box<dyn std::error::Error>> {
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(include_str!("../.github/workflows/pr-title.yaml"))?;
    let triggers = workflow
        .get("on")
        .or_else(|| workflow.get(serde_yaml::Value::Bool(true)))
        .ok_or("Missing workflow triggers")?
        .as_mapping()
        .ok_or("Invalid workflow triggers")?;
    assert!(
        triggers.contains_key(serde_yaml::Value::from("pull_request")),
        "GitHub Actions opens and updates every release pull request. \
         Only `pull_request` starts a check for those events."
    );
    assert!(
        !triggers.contains_key(serde_yaml::Value::from("pull_request_target")),
        "`pull_request_target` never starts for a release pull request. \
         A required check on that event blocks the release."
    );
    Ok(())
}

#[test]
fn release_pull_request_job_prefers_a_person_token() -> Result<(), Box<dyn std::error::Error>> {
    let workflow: serde_yaml::Value =
        serde_yaml::from_str(include_str!("../.github/workflows/release-plz.yaml"))?;
    let steps = workflow["jobs"]["release-pr"]["steps"]
        .as_sequence()
        .ok_or("Missing release pull request steps")?;
    let token = steps
        .iter()
        .filter_map(|step| step.get("env")?.get("GITHUB_TOKEN")?.as_str())
        .next()
        .ok_or("Missing release pull request token")?;
    assert!(
        token.contains("secrets.RELEASE_PLZ_TOKEN"),
        "GitHub holds every check on a pull request that GitHub Actions opens. \
         A person token starts those checks without approval."
    );
    assert!(
        token.contains("github.token"),
        "The job keeps a fallback for a repository without the secret."
    );
    Ok(())
}
