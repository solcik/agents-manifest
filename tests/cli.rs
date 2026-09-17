#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

#[path = "../src/test_support.rs"]
mod support;

struct CliFixture {
    project: tempfile::TempDir,
    cache: tempfile::TempDir,
}

impl CliFixture {
    fn new() -> Self {
        let repository = support::FixtureRepository::new();
        let body = repository.object(
            "blob",
            b"---\nname: sample\ndescription: CLI test skill\n---\nOriginal body\n",
        );
        let skill_tree = repository.tree(&[("100644", "SKILL.md", &body)]);
        let root = repository.tree(&[("40000", "sample", &skill_tree)]);
        let revision = repository.commit(&root);
        let reference = agents_manifest::manifest::SourceReference::new(
            "https://github.com/example/skills",
            revision,
            "sample",
        )
        .unwrap();
        let cache = tempfile::tempdir().unwrap();
        std::fs::rename(repository.path(), cache.path().join(reference.cache_key())).unwrap();
        let project = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .current_dir(project.path())
                .args(["init", "--quiet"])
                .status()
                .unwrap()
                .success()
        );
        std::fs::create_dir(project.path().join(".agents")).unwrap();
        let text = format!(
            "version: 1\ntargets: [codex, claude]\nskills:\n  - name: sample\n    source: {}\n    revision: '{}'\n    path: sample\n",
            reference.source(),
            reference.revision()
        );
        std::fs::write(project.path().join(".agents/skills.yaml"), text).unwrap();
        Self { project, cache }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agent-skills"));
        command
            .arg("--project")
            .arg(self.project.path())
            .arg("--cache-dir")
            .arg(self.cache.path())
            .arg("--offline");
        command
    }
}

#[test]
fn validates_example_and_returns_success() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-skills"))
        .args(["validate", "examples/skills.yaml"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Manifest validation passed.\n"
    );
}

#[test]
fn missing_manifest_returns_declaration_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-skills"))
        .args(["validate", "examples/nonexistent.yaml"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "manifest: cannot read the requested file\n"
    );
}

#[test]
fn ordinary_directory_sync_preserves_ownership_and_removal() {
    let fixture = CliFixture::new();
    std::fs::remove_dir_all(fixture.project.path().join(".git")).unwrap();
    for operation in ["plan", "sync", "check", "sync"] {
        let output = fixture
            .command()
            .args([operation, "--json"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let skill = fixture
        .project
        .path()
        .join(".agents/skills/sample/SKILL.md");
    std::fs::write(&skill, "User content").unwrap();
    let output = fixture.command().args(["sync", "--json"]).output().unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(std::fs::read_to_string(&skill).unwrap(), "User content");
    std::fs::write(
        &skill,
        b"---\nname: sample\ndescription: CLI test skill\n---\nOriginal body\n",
    )
    .unwrap();
    std::fs::write(
        fixture.project.path().join(".agents/skills.yaml"),
        "version: 1\ntargets: [codex, claude]\nskills: []\n",
    )
    .unwrap();
    let output = fixture.command().args(["sync", "--json"]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!skill.exists());
}

#[test]
fn broken_repository_metadata_is_not_an_ordinary_directory() {
    let fixture = CliFixture::new();
    std::fs::remove_dir_all(fixture.project.path().join(".git")).unwrap();
    std::fs::write(
        fixture.project.path().join(".git"),
        "gitdir: missing-repository\n",
    )
    .unwrap();
    let output = fixture.command().args(["sync", "--json"]).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        !fixture
            .project
            .path()
            .join(".agents/skills-state.json")
            .exists()
    );
}

#[test]
fn real_cli_plans_syncs_checks_and_reports_drift_without_credentials() {
    let fixture = CliFixture::new();
    let output = fixture.command().args(["plan", "--json"]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["changes"].as_array().unwrap().len(), 4);
    assert!(
        !fixture
            .project
            .path()
            .join(".agents/skills-state.json")
            .exists()
    );
    let output = fixture.command().args(["sync", "--json"]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        fixture
            .command()
            .args(["check", "--quiet"])
            .status()
            .unwrap()
            .success()
    );
    let output = fixture.command().args(["sync", "--json"]).output().unwrap();
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["changes"], serde_json::json!([]));
    std::fs::write(
        fixture
            .project
            .path()
            .join(".agents/skills/sample/SKILL.md"),
        "User content",
    )
    .unwrap();
    let output = fixture
        .command()
        .args(["check", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    let output = fixture.command().args(["sync", "--json"]).output().unwrap();
    assert_eq!(output.status.code(), Some(4));
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], 4);
    assert_eq!(
        std::fs::read_to_string(
            fixture
                .project
                .path()
                .join(".agents/skills/sample/SKILL.md")
        )
        .unwrap(),
        "User content"
    );
}

#[test]
fn validation_needs_neither_git_nor_a_cache() {
    let fixture = CliFixture::new();
    let output = fixture
        .command()
        .env("PATH", "")
        .args(["validate", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!({"version":1,"valid":true})
    );
}

#[test]
fn invalid_manifest_fails_before_project_lock_creation() {
    let fixture = CliFixture::new();
    std::fs::write(
        fixture.project.path().join(".agents/skills.yaml"),
        "version: 2\ntargets: [codex]\nskills: []\n",
    )
    .unwrap();
    let output = fixture
        .command()
        .env("PATH", "")
        .arg("sync")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!fixture.project.path().join(".agents/skills.lock").exists());
}

#[test]
fn completion_output_handles_a_closed_pipe() {
    use std::process::Stdio;
    let mut child = Command::new(env!("CARGO_BIN_EXE_agent-skills"))
        .args(["completions", "zsh"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    assert!(child.wait().unwrap().success());
}

#[test]
fn tracked_journals_cannot_trigger_recovery_of_project_files() {
    let fixture = CliFixture::new();
    let root = fixture.project.path();
    std::fs::write(root.join(".gitignore"), "User rules\n").unwrap();
    std::fs::create_dir(root.join(".agents/skills-transaction")).unwrap();
    let journal = serde_json::json!({"version":1,"entries":[{
        "path":".gitignore", "before":null,
        "after":{"hash":blake3::hash(b"User rules\n").to_hex().to_string(), "directory":false}
    }]});
    std::fs::write(
        root.join(".agents/skills-transaction/journal.json"),
        serde_json::to_vec(&journal).unwrap(),
    )
    .unwrap();
    assert!(
        Command::new("git")
            .current_dir(root)
            .args(["add", ".agents/skills-transaction"])
            .status()
            .unwrap()
            .success()
    );
    let output = fixture.command().arg("sync").output().unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(
        std::fs::read_to_string(root.join(".gitignore")).unwrap(),
        "User rules\n"
    );
    assert!(!root.join(".agents/skills.lock").exists());
}
