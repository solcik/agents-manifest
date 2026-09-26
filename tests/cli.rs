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

/// Build a worktree container: a bare directory plus one worktree per branch.
///
/// The layout matches the convention this CLI must serve.
/// `<container>/.git` is a FILE that points at `<container>/.bare`.
struct ContainerFixture {
    container: tempfile::TempDir,
    cache: tempfile::TempDir,
}

impl ContainerFixture {
    fn new(lanes: &[&str]) -> Self {
        let inner = CliFixture::new();
        let seed = inner.project.path();
        run_git(seed, &["add", "-A"]);
        run_git(
            seed,
            &[
                "-c",
                "user.email=fixture@example.test",
                "-c",
                "user.name=Fixture",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--quiet",
                "-m",
                "seed",
            ],
        );
        let container = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .args(["clone", "--quiet", "--bare"])
                .arg(seed)
                .arg(container.path().join(".bare"))
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(container.path().join(".git"), "gitdir: ./.bare\n").unwrap();
        for (index, lane) in lanes.iter().enumerate() {
            let mut args = vec!["worktree", "add", "--quiet"];
            if index > 0 {
                args.push("-b");
                args.push(lane);
            }
            args.push(lane);
            if index == 0 {
                args.push("HEAD");
            }
            run_git(container.path(), &args);
        }
        Self {
            container,
            cache: inner.cache,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agent-skills"));
        command
            .arg("--project")
            .arg(self.container.path())
            .arg("--cache-dir")
            .arg(self.cache.path())
            .arg("--offline");
        command
    }
}

fn run_git(directory: &std::path::Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .current_dir(directory)
            .args(args)
            .status()
            .unwrap()
            .success(),
        "git {args:?} failed in {}",
        directory.display()
    );
}

#[test]
fn worktrees_publish_one_container_manifest_into_every_lane() {
    let fixture = ContainerFixture::new(&["develop", "lane"]);
    let container = fixture.container.path();
    // The container manifest is the single declaration for every lane.
    std::fs::create_dir_all(container.join(".agents")).unwrap();
    std::fs::copy(
        container.join("develop/.agents/skills.yaml"),
        container.join(".agents/skills.yaml"),
    )
    .unwrap();

    let output = fixture
        .command()
        .args(["sync", "--worktrees"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    for lane in ["develop", "lane"] {
        let skill = container.join(lane).join(".agents/skills/sample/SKILL.md");
        assert!(skill.is_file(), "missing projection in {lane}");
        // Each lane owns its state and ignore block independently.
        assert!(
            container
                .join(lane)
                .join(".agents/skills-state.json")
                .is_file()
        );
    }
    // The container itself is not a worktree and receives no projection.
    assert!(!container.join(".agents/skills/sample").exists());

    let check = fixture
        .command()
        .args(["check", "--worktrees"])
        .output()
        .unwrap();
    assert!(check.status.success(), "{:?}", check);
}

#[test]
fn worktrees_report_each_lane_separately_in_json() {
    let fixture = ContainerFixture::new(&["develop", "lane"]);
    let container = fixture.container.path();
    std::fs::create_dir_all(container.join(".agents")).unwrap();
    std::fs::copy(
        container.join("develop/.agents/skills.yaml"),
        container.join(".agents/skills.yaml"),
    )
    .unwrap();

    let output = fixture
        .command()
        .args(["plan", "--worktrees", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let projects = report["projects"].as_array().unwrap();
    assert_eq!(projects.len(), 2);
    for project in projects {
        let path = project["path"].as_str().unwrap();
        assert!(
            path.ends_with("/develop") || path.ends_with("/lane"),
            "{path}"
        );
        assert!(!project["changes"].as_array().unwrap().is_empty());
    }
    // Planning writes nothing.
    assert!(!container.join("develop/.agents/skills/sample").exists());
}

#[test]
fn a_single_lane_stays_the_default_and_leaves_its_siblings_alone() {
    let fixture = ContainerFixture::new(&["develop", "lane"]);
    let container = fixture.container.path();
    let output = Command::new(env!("CARGO_BIN_EXE_agent-skills"))
        .arg("--project")
        .arg(container.join("develop"))
        .arg("--cache-dir")
        .arg(fixture.cache.path())
        .args(["--offline", "sync"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert!(
        container
            .join("develop/.agents/skills/sample/SKILL.md")
            .is_file()
    );
    assert!(!container.join("lane/.agents/skills/sample").exists());
}

#[test]
fn worktrees_outside_a_repository_are_refused() {
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(elsewhere.path().join(".agents")).unwrap();
    std::fs::write(
        elsewhere.path().join(".agents/skills.yaml"),
        "version: 1\ntargets: [codex]\nskills: []\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_agent-skills"))
        .arg("--project")
        .arg(elsewhere.path())
        .args(["--offline", "plan", "--worktrees"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

/// A container whose `base` lane holds the bare HEAD branch, synced once.
fn linked_container() -> ContainerFixture {
    let fixture = ContainerFixture::new(&["lane"]);
    let container = fixture.container.path();
    let output = Command::new("git")
        .current_dir(container)
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .unwrap();
    let branch = String::from_utf8(output.stdout).unwrap();
    run_git(
        container,
        &["worktree", "add", "--quiet", "base", branch.trim()],
    );
    let sync = Command::new(env!("CARGO_BIN_EXE_agent-skills"))
        .arg("--project")
        .arg(container.join("base"))
        .arg("--cache-dir")
        .arg(fixture.cache.path())
        .args(["--offline", "sync"])
        .output()
        .unwrap();
    assert!(sync.status.success(), "{:?}", sync);
    fixture
}

fn link_command(project: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agent-skills"));
    command.arg("--project").arg(project).arg("link");
    command
}

#[test]
fn link_points_the_container_root_at_the_base_worktree() {
    let fixture = linked_container();
    let container = fixture.container.path();

    let output = link_command(container).output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    for directory in [".agents/skills", ".claude/skills"] {
        let path = container.join(directory);
        assert_eq!(
            std::fs::read_link(&path).unwrap(),
            std::path::Path::new("../base").join(directory)
        );
        // The root reads the base worktree's published skill through the link.
        assert!(path.join("sample/SKILL.md").is_file());
    }
    // The root receives no projection state of its own.
    assert!(!container.join(".agents/skills-state.json").exists());

    let again = link_command(container).output().unwrap();
    assert!(again.status.success(), "{:?}", again);
    assert_eq!(
        String::from_utf8_lossy(&again.stdout),
        "Container links are up to date.\n"
    );
    let check = link_command(container).arg("--check").output().unwrap();
    assert!(check.status.success(), "{:?}", check);
}

#[test]
fn link_runs_from_any_lane_of_the_container() {
    let fixture = linked_container();
    let container = fixture.container.path();
    let output = link_command(&container.join("lane")).output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert!(container.join(".agents/skills/sample/SKILL.md").is_file());
}

#[test]
fn link_moves_a_link_that_points_at_another_lane() {
    let fixture = linked_container();
    let container = fixture.container.path();
    std::fs::create_dir_all(container.join(".agents")).unwrap();
    std::os::unix::fs::symlink("../lane/.agents/skills", container.join(".agents/skills")).unwrap();
    let output = link_command(container).output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        std::fs::read_link(container.join(".agents/skills")).unwrap(),
        std::path::Path::new("../base/.agents/skills")
    );
}

#[test]
fn link_check_reports_a_missing_link_without_writing() {
    let fixture = linked_container();
    let container = fixture.container.path();
    let output = link_command(container).arg("--check").output().unwrap();
    assert_eq!(output.status.code(), Some(5), "{:?}", output);
    assert!(!container.join(".agents/skills").exists());
}

#[test]
fn link_never_replaces_a_real_directory_in_the_root() {
    let fixture = linked_container();
    let container = fixture.container.path();
    std::fs::create_dir_all(container.join(".claude/skills/mine")).unwrap();
    let output = link_command(container).output().unwrap();
    assert_eq!(output.status.code(), Some(4), "{:?}", output);
    assert!(container.join(".claude/skills/mine").is_dir());
    // The command decides every link before the first write.
    assert!(!container.join(".agents/skills").exists());
}

#[test]
fn link_accepts_an_explicit_base_worktree() {
    let fixture = linked_container();
    let container = fixture.container.path();
    let sync = Command::new(env!("CARGO_BIN_EXE_agent-skills"))
        .arg("--project")
        .arg(container.join("lane"))
        .arg("--cache-dir")
        .arg(fixture.cache.path())
        .args(["--offline", "sync"])
        .output()
        .unwrap();
    assert!(sync.status.success(), "{:?}", sync);
    let output = link_command(container)
        .arg("--base")
        .arg(container.join("lane"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        std::fs::read_link(container.join(".agents/skills")).unwrap(),
        std::path::Path::new("../lane/.agents/skills")
    );
}

#[test]
fn link_outside_a_container_is_refused() {
    let fixture = CliFixture::new();
    let output = link_command(fixture.project.path()).output().unwrap();
    assert_eq!(output.status.code(), Some(2), "{:?}", output);
}
