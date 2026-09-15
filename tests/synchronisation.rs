#![allow(clippy::unwrap_used, clippy::expect_used)]

use agents_manifest::{
    content::{FileContent, Tree},
    error::{Error, Result},
    manifest::{self, Manifest, Skill, SourceReference},
    plan::{Planner, STATE_PATH},
    source::Source,
    transaction::{ProjectLock, ProjectReadLock, Transaction},
};
use std::{collections::BTreeMap, fs, process::Command, sync::Arc};

#[derive(Default)]
struct MemorySource {
    skills: BTreeMap<String, String>,
    bundles: BTreeMap<String, Vec<u8>>,
}

impl MemorySource {
    fn sample(text: &str) -> Self {
        Self {
            skills: [("sample".into(), text.into())].into(),
            bundles: BTreeMap::new(),
        }
    }
}

impl Source for MemorySource {
    fn file(&self, reference: &SourceReference) -> Result<Vec<u8>> {
        self.bundles
            .get(reference.path())
            .cloned()
            .ok_or_else(|| Error::Source("fixture: missing bundle".into()))
    }

    fn skill(&self, skill: &Skill) -> Result<Tree> {
        let text = self
            .skills
            .get(&skill.name)
            .ok_or_else(|| Error::Source("fixture: missing skill".into()))?;
        let body = format!(
            "---\nname: {}\ndescription: A test skill\n---\n{text}\n",
            skill.name
        );
        let mut tree = Tree::new();
        tree.insert(
            "SKILL.md".into(),
            FileContent {
                bytes: Arc::from(body.into_bytes()),
                executable: false,
            },
        );
        tree.insert(
            "scripts/run.sh".into(),
            FileContent {
                bytes: Arc::from(&b"#!/bin/sh\nexit 0\n"[..]),
                executable: true,
            },
        );
        Ok(tree)
    }
}

fn project() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .current_dir(directory.path())
            .args(["init", "--quiet"])
            .status()
            .unwrap()
            .success()
    );
    directory
}

fn manifest(skills: &str) -> agents_manifest::manifest::ValidatedManifest {
    let text = format!("version: 1\ntargets: [codex, claude, pi, opencode]\nskills: {skills}\n");
    manifest::parse::<Manifest>(text.as_bytes(), "fixture")
        .unwrap()
        .validate()
        .unwrap()
}

fn sample_manifest() -> agents_manifest::manifest::ValidatedManifest {
    manifest(
        "\n  - name: sample\n    source: https://github.com/example/skills\n    revision: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n    path: skills/sample",
    )
}

#[test]
fn sync_is_idempotent_copies_harness_files_and_removes_owned_output() {
    let project = project();
    let root = project.path();
    fs::write(root.join(".gitignore"), "# User content\n/cache\n").unwrap();
    let input = sample_manifest();
    let source = MemorySource::sample("First version");
    let _lock = ProjectLock::acquire(root).unwrap();
    let plan = Planner::new(root, &input, &source).build().unwrap();
    Transaction::new(&_lock).apply(&plan).unwrap();
    assert_eq!(
        fs::read(root.join(".agents/skills/sample/SKILL.md")).unwrap(),
        fs::read(root.join(".claude/skills/sample/SKILL.md")).unwrap()
    );
    assert!(
        fs::read_to_string(root.join(".gitignore"))
            .unwrap()
            .starts_with("# User content\n/cache\n")
    );
    assert!(
        Planner::new(root, &input, &source)
            .build()
            .unwrap()
            .operations
            .is_empty()
    );
    let removed = Planner::new(root, &manifest("[]"), &source)
        .build()
        .unwrap();
    Transaction::new(&_lock).apply(&removed).unwrap();
    assert!(!root.join(".agents/skills/sample").exists());
    assert!(!root.join(".claude/skills/sample").exists());
    assert!(
        Planner::new(root, &manifest("[]"), &source)
            .build()
            .unwrap()
            .operations
            .is_empty()
    );
}

#[test]
fn changed_generated_files_conflict_and_check_reports_drift() {
    let project = project();
    let root = project.path();
    let input = sample_manifest();
    let source = MemorySource::sample("First version");
    let _lock = ProjectLock::acquire(root).unwrap();
    let plan = Planner::new(root, &input, &source).build().unwrap();
    Transaction::new(&_lock).apply(&plan).unwrap();
    let state = fs::read(root.join(STATE_PATH)).unwrap();
    fs::write(root.join(".agents/skills/sample/SKILL.md"), "User edit").unwrap();
    assert!(matches!(
        Planner::new(root, &input, &source).build(),
        Err(Error::Conflict(_))
    ));
    assert!(
        !Planner::new(root, &input, &source)
            .check(true)
            .build()
            .unwrap()
            .operations
            .is_empty()
    );
    assert_eq!(fs::read(root.join(STATE_PATH)).unwrap(), state);
    assert_eq!(
        fs::read_to_string(root.join(".agents/skills/sample/SKILL.md")).unwrap(),
        "User edit"
    );
    assert!(
        Planner::new(root, &manifest("[]"), &source)
            .build()
            .is_err()
    );
}

#[test]
fn failed_resolution_preserves_the_previous_projection() {
    let project = project();
    let root = project.path();
    let input = sample_manifest();
    let source = MemorySource::sample("First version");
    let _lock = ProjectLock::acquire(root).unwrap();
    let plan = Planner::new(root, &input, &source).build().unwrap();
    Transaction::new(&_lock).apply(&plan).unwrap();
    let state = fs::read(root.join(STATE_PATH)).unwrap();
    assert!(matches!(
        Planner::new(root, &input, &MemorySource::default()).build(),
        Err(Error::Source(_))
    ));
    assert_eq!(fs::read(root.join(STATE_PATH)).unwrap(), state);
}

#[test]
fn local_collision_and_unmanaged_mirror_preserve_local_files() {
    for relative in [".agents/skills/sample", ".claude/skills/sample"] {
        let project = project();
        let root = project.path();
        fs::create_dir_all(root.join(relative)).unwrap();
        fs::write(root.join(relative).join("SKILL.md"), "Local content").unwrap();
        assert!(matches!(
            Planner::new(root, &sample_manifest(), &MemorySource::sample("External")).build(),
            Err(Error::Conflict(_))
        ));
        assert_eq!(
            fs::read_to_string(root.join(relative).join("SKILL.md")).unwrap(),
            "Local content"
        );
    }
}

#[test]
fn copies_local_skills_without_owning_their_canonical_directory() {
    let project = project();
    let root = project.path();
    fs::create_dir_all(root.join(".agents/skills/local")).unwrap();
    let text = "---\nname: local\ndescription: Local project rules\n---\nLocal body\n";
    fs::write(root.join(".agents/skills/local/SKILL.md"), text).unwrap();
    assert!(
        Command::new("git")
            .current_dir(root)
            .args(["add", ".agents/skills/local"])
            .status()
            .unwrap()
            .success()
    );
    let input = manifest("[]");
    let source = MemorySource::default();
    let _lock = ProjectLock::acquire(root).unwrap();
    let plan = Planner::new(root, &input, &source).build().unwrap();
    Transaction::new(&_lock).apply(&plan).unwrap();
    assert_eq!(
        fs::read_to_string(root.join(".claude/skills/local/SKILL.md")).unwrap(),
        text
    );
    let state = agents_manifest::plan::load_state(root).unwrap();
    assert!(!state.outputs.contains_key(".agents/skills/local"));
    assert!(state.outputs.contains_key(".claude/skills/local"));
}

#[test]
fn plan_never_writes_project_files_and_detects_tracked_output() {
    let project = project();
    let root = project.path();
    let input = sample_manifest();
    let source = MemorySource::sample("External");
    Planner::new(root, &input, &source).build().unwrap();
    assert!(!root.join(".agents").exists());
    fs::create_dir_all(root.join(".claude/skills/sample")).unwrap();
    fs::write(root.join(".claude/skills/sample/SKILL.md"), "Tracked").unwrap();
    assert!(
        Command::new("git")
            .current_dir(root)
            .args(["add", ".claude"])
            .status()
            .unwrap()
            .success()
    );
    assert!(matches!(
        Planner::new(root, &input, &source).build(),
        Err(Error::Conflict(_))
    ));
}

#[test]
fn changed_project_after_planning_stops_before_publication() {
    let project = project();
    let root = project.path();
    let input = sample_manifest();
    let source = MemorySource::sample("External");
    let _lock = ProjectLock::acquire(root).unwrap();
    let plan = Planner::new(root, &input, &source).build().unwrap();
    fs::write(root.join(".gitignore"), "New user rules\n").unwrap();
    assert!(matches!(
        Transaction::new(&_lock).apply(&plan),
        Err(Error::Conflict(_))
    ));
    assert!(!root.join(".agents/skills/sample").exists());
    assert_eq!(
        fs::read_to_string(root.join(".gitignore")).unwrap(),
        "New user rules\n"
    );
}

#[test]
fn bundles_cannot_nest_or_duplicate_names() {
    let project = project();
    let text = b"version: 1\ntargets: [codex]\nskills: []\nbundles:\n  - source: https://github.com/example/skills\n    revision: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n    path: team.yaml\n";
    let input = manifest::parse::<Manifest>(text, "fixture")
        .unwrap()
        .validate()
        .unwrap();
    let mut source = MemorySource::sample("External");
    source.bundles.insert(
        "team.yaml".into(),
        b"version: 1\nskills: []\nbundles: []\n".to_vec(),
    );
    assert!(matches!(
        Planner::new(project.path(), &input, &source).build(),
        Err(Error::Invalid(_))
    ));
    let entry = "  - name: sample\n    source: https://github.com/example/skills\n    revision: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n    path: skills/sample\n";
    source.bundles.insert(
        "team.yaml".into(),
        format!("version: 1\nskills:\n{entry}{entry}").into_bytes(),
    );
    assert!(matches!(
        Planner::new(project.path(), &input, &source).build(),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn project_lock_rejects_competing_sync() {
    let project = project();
    let _first = ProjectLock::acquire(project.path()).unwrap();
    assert!(ProjectLock::acquire(project.path()).is_err());
    assert!(ProjectReadLock::acquire(project.path()).is_err());
}

#[cfg(unix)]
#[test]
fn symlinked_output_ancestors_never_modify_the_target() {
    use std::os::unix::fs::symlink;
    let project = project();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), project.path().join(".agents")).unwrap();
    assert!(
        Planner::new(
            project.path(),
            &sample_manifest(),
            &MemorySource::sample("External")
        )
        .build()
        .is_err()
    );
    assert!(ProjectLock::acquire(project.path()).is_err());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn canonical_only_projects_validate_local_skill_identity() {
    let project = project();
    let root = project.path();
    fs::create_dir_all(root.join(".agents/skills/local")).unwrap();
    fs::write(
        root.join(".agents/skills/local/SKILL.md"),
        "---\nname: sample\ndescription: Conflicting identity\n---\nBody\n",
    )
    .unwrap();
    let input =
        manifest::parse::<Manifest>(b"version: 1\ntargets: [codex]\nskills: []\n", "fixture")
            .unwrap()
            .validate()
            .unwrap();
    assert!(matches!(
        Planner::new(root, &input, &MemorySource::default()).build(),
        Err(Error::Conflict(_))
    ));
}
