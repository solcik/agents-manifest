use crate::{
    content::{self, FileContent, Fingerprint, Tree},
    error::{Error, IoContext, Result},
    manifest::{self, Bundle, Skill, SourceReference, Target, ValidatedManifest},
    source::{Source, validate_frontmatter},
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
    sync::Arc,
};

pub const STATE_PATH: &str = ".agents/skills-state.json";
pub const TRANSACTION_PATH: &str = ".agents/skills-transaction";
pub const LOCK_PATH: &str = ".agents/skills.lock";
const BEGIN: &str = "# BEGIN agents-manifest managed output";
const END: &str = "# END agents-manifest managed output";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Owned {
    pub fingerprint: Fingerprint,
    pub origin: SkillOrigin,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    content = "reference",
    rename_all = "lowercase",
    deny_unknown_fields
)]
pub enum SkillOrigin {
    External(SourceReference),
    Local { path: String },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub version: u32,
    pub outputs: BTreeMap<String, Owned>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: 1,
            outputs: BTreeMap::new(),
        }
    }
}

pub fn output_path(path: &str) -> bool {
    [".agents/skills/", ".claude/skills/"].iter().any(|prefix| {
        path.strip_prefix(prefix)
            .is_some_and(|suffix| manifest::name(suffix).is_ok())
    })
}

pub fn operation_path(path: &str) -> bool {
    output_path(path) || matches!(path, STATE_PATH | ".gitignore")
}

pub fn load_state(root: &Path) -> Result<State> {
    let state = match content::read_project_file(root, STATE_PATH, content::MAX_FILE)? {
        Some(bytes) => serde_json::from_slice::<State>(&bytes)
            .map_err(|_| Error::Conflict("state: invalid ownership metadata".into()))?,
        None => State::default(),
    };
    if state.version != 1 || state.outputs.len() > content::MAX_FILES {
        return Err(Error::Conflict(
            "state: unsupported version or excessive entry count".into(),
        ));
    }
    for (path, owned) in &state.outputs {
        if !output_path(path)
            || !owned.fingerprint.directory
            || !valid_hash(&owned.fingerprint.hash)
        {
            return Err(Error::Conflict(
                "state: invalid managed output entry".into(),
            ));
        }
    }
    Ok(state)
}

pub fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit())
}

#[derive(Clone)]
pub enum Payload {
    Tree(Arc<Tree>),
    File(Arc<[u8]>),
    Remove,
}

pub struct Operation {
    pub path: String,
    pub before: Option<Fingerprint>,
    pub after: Option<Fingerprint>,
    pub payload: Payload,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Create,
    Update,
    Remove,
}

#[derive(Clone, Debug, Serialize)]
pub struct Change {
    pub action: Action,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub version: u32,
    pub changes: Vec<Change>,
}

pub struct Plan {
    pub operations: Vec<Operation>,
    pub report: Report,
}

impl Operation {
    pub fn change(&self) -> Change {
        Change {
            action: if self.after.is_none() {
                Action::Remove
            } else if self.before.is_none() {
                Action::Create
            } else {
                Action::Update
            },
            path: self.path.clone(),
        }
    }
}

impl Plan {
    fn new(operations: Vec<Operation>) -> Self {
        let report = Report {
            version: 1,
            changes: operations.iter().map(Operation::change).collect(),
        };
        Self { operations, report }
    }
}

struct OutputSnapshot {
    path: String,
    current: Option<Fingerprint>,
    previous: Option<Owned>,
}

impl OutputSnapshot {
    fn reconcile(&self, desired: Option<&Desired>, check: bool) -> Result<Option<Operation>> {
        let after = desired.map(|item| item.owner.fingerprint.clone());
        if self.current.is_some() && self.previous.is_none() {
            return Err(Error::Conflict(format!(
                "{}: existing directory has no ownership record",
                self.path
            )));
        }
        if !check
            && self.current.is_some()
            && self.previous.as_ref().map(|p| &p.fingerprint) != self.current.as_ref()
            && self.current != after
        {
            return Err(Error::Conflict(format!(
                "{}: generated content has local changes",
                self.path
            )));
        }
        Ok((self.current != after).then(|| Operation {
            path: self.path.clone(),
            before: self.current.clone(),
            after,
            payload: desired
                .map(|item| Payload::Tree(item.tree.clone()))
                .unwrap_or(Payload::Remove),
        }))
    }
}

pub struct Planner<'a, S> {
    root: &'a Path,
    input: &'a ValidatedManifest,
    source: &'a S,
    jobs: usize,
    check: bool,
}

impl<'a, S: Source> Planner<'a, S> {
    pub fn new(root: &'a Path, input: &'a ValidatedManifest, source: &'a S) -> Self {
        Self {
            root,
            input,
            source,
            jobs: 4,
            check: false,
        }
    }
    pub fn jobs(mut self, jobs: usize) -> Self {
        self.jobs = jobs;
        self
    }
    pub fn check(mut self, check: bool) -> Self {
        self.check = check;
        self
    }
    pub fn build(self) -> Result<Plan> {
        let Self {
            root,
            input,
            source,
            jobs,
            check,
        } = self;
        content::safe_ancestors(root, TRANSACTION_PATH)?;
        if root.join(TRANSACTION_PATH).exists() {
            return Err(Error::Conflict(
                "transaction: run sync to recover interrupted publication".into(),
            ));
        }
        let state = load_state(root)?;
        let tracked = tracked(root)?;
        validate_metadata_paths(&tracked)?;
        let locals = local_skills(root, &state)?;
        if !(1..=64).contains(&jobs) {
            return Err(Error::Invalid(
                "jobs: select between 1 and 64 workers".into(),
            ));
        }
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(jobs)
            .build()
            .map_err(|_| Error::Internal("cannot create resolver worker pool".into()))?;
        let resolved = resolve(input, source, &pool)?;
        for ResolvedSkill { skill, .. } in &resolved {
            if locals.binary_search(&skill.name).is_ok() {
                return Err(Error::Conflict(format!(
                    "{}: local and external skill names collide",
                    skill.name
                )));
            }
        }
        let budget = std::sync::atomic::AtomicUsize::new(0);
        let trees: Vec<Result<Tree>> = pool.install(|| {
            resolved
                .par_iter()
                .map(|ResolvedSkill { skill, .. }| {
                    let tree = source.skill(skill)?;
                    tree.validate()?;
                    let size = tree.byte_len();
                    let used = budget.fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                    if size > 512 * 1024 * 1024 || used > 512 * 1024 * 1024 - size {
                        return Err(Error::Source(
                            "project: resolved content exceeds 512 MiB".into(),
                        ));
                    }
                    Ok(tree)
                })
                .collect()
        });
        let mut desired = BTreeMap::<String, Desired>::new();
        let mut total = 0usize;
        for (ResolvedSkill { skill, targets }, tree) in resolved.into_iter().zip(trees) {
            let tree = Arc::new(tree?);
            total += tree.byte_len();
            if total > 512 * 1024 * 1024 {
                return Err(Error::Source(
                    "project: resolved content exceeds 512 MiB".into(),
                ));
            }
            let owner = Owned {
                fingerprint: tree.fingerprint(),
                origin: SkillOrigin::External(skill.reference),
            };
            desired.insert(
                format!(".agents/skills/{}", skill.name),
                Desired {
                    tree: tree.clone(),
                    owner: owner.clone(),
                },
            );
            if targets.contains(&Target::Claude) {
                desired.insert(
                    format!(".claude/skills/{}", skill.name),
                    Desired { tree, owner },
                );
            }
        }
        if input.targets.contains(&Target::Claude) {
            for name in locals {
                let tree = Arc::new(read_local_tree(root, &name)?);
                total += tree.byte_len();
                if total > 512 * 1024 * 1024 {
                    return Err(Error::Source(
                        "project: resolved content exceeds 512 MiB".into(),
                    ));
                }
                let owner = Owned {
                    fingerprint: tree.fingerprint(),
                    origin: SkillOrigin::Local {
                        path: format!(".agents/skills/{name}"),
                    },
                };
                desired.insert(format!(".claude/skills/{name}"), Desired { tree, owner });
            }
        }
        let next = State {
            version: 1,
            outputs: desired
                .iter()
                .map(|(path, item)| (path.clone(), item.owner.clone()))
                .collect(),
        };
        let paths: BTreeSet<_> = state
            .outputs
            .keys()
            .chain(desired.keys())
            .cloned()
            .collect();
        let snapshots = paths
            .into_iter()
            .map(|path| {
                refuse_tracked(&path, &tracked)?;
                Ok(OutputSnapshot {
                    current: content::fingerprint(root, &path)?,
                    previous: state.outputs.get(&path).cloned(),
                    path,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut operations = snapshots
            .iter()
            .map(|snapshot| snapshot.reconcile(desired.get(&snapshot.path), check))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let ignore = ignore_content(root, next.outputs.keys())?;
        add_file(root, ".gitignore", ignore.into_bytes(), &mut operations)?;
        let mut state_bytes = serde_json::to_vec_pretty(&next)
            .map_err(|_| Error::Internal("cannot encode ownership state".into()))?;
        state_bytes.push(b'\n');
        add_file(root, STATE_PATH, state_bytes, &mut operations)?;
        if root.join(TRANSACTION_PATH).exists() || load_state(root)? != state {
            return Err(Error::Conflict(
                "transaction: project changed during planning".into(),
            ));
        }
        Ok(Plan::new(operations))
    }
}

fn resolve<S: Source>(
    input: &ValidatedManifest,
    source: &S,
    pool: &rayon::ThreadPool,
) -> Result<Vec<ResolvedSkill>> {
    let bundles: Vec<Result<Vec<ResolvedSkill>>> = pool.install(|| {
        input
            .manifest
            .bundles
            .par_iter()
            .map(|reference| {
                let parent = reference.effective_targets(&input.targets)?;
                let bytes = source.file(&reference.reference)?;
                let bundle: Bundle = manifest::parse(&bytes, "bundle")?;
                if bundle.version != 1 {
                    return Err(Error::Invalid(
                        "bundle.version: only version 1 is supported".into(),
                    ));
                }
                if bundle.skills.len() > 1000 {
                    return Err(Error::Invalid("bundle: maximum skill count is 1000".into()));
                }
                bundle
                    .skills
                    .into_iter()
                    .map(|skill| ResolvedSkill::new(skill, &parent))
                    .collect()
            })
            .collect()
    });
    let mut skills = input
        .manifest
        .skills
        .iter()
        .map(|skill| ResolvedSkill::new(skill.clone(), &input.targets))
        .collect::<Result<Vec<_>>>()?;
    for bundle in bundles {
        skills.extend(bundle?);
    }
    if skills.len() > 1000 {
        return Err(Error::Invalid(
            "manifest: maximum resolved skill count is 1000".into(),
        ));
    }
    skills.sort_by(|a, b| a.skill.name.cmp(&b.skill.name));
    for pair in skills.windows(2) {
        if pair[0].skill.name == pair[1].skill.name {
            return Err(Error::Invalid(format!(
                "{}: duplicate resolved skill name",
                pair[0].skill.name
            )));
        }
    }
    Ok(skills)
}

fn tracked(root: &Path) -> Result<BTreeSet<String>> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "-z", "--", "."])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .context("inspect tracked project files")?;
    if !output.status.success() {
        return Err(Error::Invalid("project: require a Git working tree".into()));
    }
    output
        .stdout
        .split(|b| *b == 0)
        .filter(|path| !path.is_empty())
        .map(|path| {
            std::str::from_utf8(path)
                .map(str::to_owned)
                .map_err(|_| Error::Conflict("project: require UTF-8 tracked paths".into()))
        })
        .collect()
}

fn refuse_tracked(path: &str, tracked: &BTreeSet<String>) -> Result<()> {
    let prefix = format!("{path}/");
    if tracked.contains(path)
        || tracked
            .range(prefix.clone()..)
            .next()
            .is_some_and(|file| file.starts_with(&prefix))
    {
        return Err(Error::Conflict(format!(
            "{path}: tracked files cannot become generated output"
        )));
    }
    Ok(())
}

fn validate_metadata_paths(tracked: &BTreeSet<String>) -> Result<()> {
    for path in [STATE_PATH, LOCK_PATH, TRANSACTION_PATH] {
        refuse_tracked(path, tracked)?;
    }
    Ok(())
}

pub(crate) fn validate_operation_ownership<'a>(
    root: &Path,
    paths: impl Iterator<Item = &'a str>,
) -> Result<()> {
    let tracked = tracked(root)?;
    validate_metadata_paths(&tracked)?;
    for path in paths {
        if path != ".gitignore" {
            refuse_tracked(path, &tracked)?;
        }
    }
    Ok(())
}

struct Desired {
    tree: Arc<Tree>,
    owner: Owned,
}

struct ResolvedSkill {
    skill: Skill,
    targets: BTreeSet<Target>,
}

impl ResolvedSkill {
    fn new(skill: Skill, parent: &BTreeSet<Target>) -> Result<Self> {
        let targets = skill.effective_targets(parent)?;
        Ok(Self { skill, targets })
    }
}

fn local_skills(root: &Path, state: &State) -> Result<Vec<String>> {
    content::safe_ancestors(root, ".agents/skills")?;
    let mut names = Vec::new();
    let directory = root.join(".agents/skills");
    if !directory.exists() {
        return Ok(names);
    }
    for entry in fs::read_dir(directory).context("read local skills")? {
        let entry = entry.context("read local skill entry")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| Error::Conflict("local skills: require UTF-8 names".into()))?;
        if state
            .outputs
            .contains_key(&format!(".agents/skills/{name}"))
        {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path()).context("inspect local skill")?;
        if metadata.is_file() {
            continue;
        }
        if !metadata.is_dir() {
            return Err(Error::Conflict(format!(
                ".agents/skills/{name}: require a regular skill directory"
            )));
        }
        manifest::name(&name)?;
        let body_path = format!(".agents/skills/{name}/SKILL.md");
        let body = content::read_project_file(root, &body_path, content::MAX_FILE)?
            .ok_or_else(|| Error::Conflict(format!("{name}: local skill lacks SKILL.md")))?;
        validate_frontmatter(&body, &name).map_err(|_| {
            Error::Conflict(format!("{name}: invalid local skill name or frontmatter"))
        })?;
        names.push(name);
        if names.len() > 1000 {
            return Err(Error::Conflict(
                "local skills: maximum count is 1000".into(),
            ));
        }
    }
    names.sort();
    Ok(names)
}

fn read_local_tree(root: &Path, name: &str) -> Result<Tree> {
    let relative = format!(".agents/skills/{name}");
    content::fingerprint(root, &relative)?;
    fn visit(root: &Path, path: &Path, tree: &mut Tree) -> Result<()> {
        for entry in fs::read_dir(path).context("read local skill files")? {
            let entry = entry.context("read local skill file")?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).context("inspect local skill file")?;
            if metadata.is_dir() {
                visit(root, &path, tree)?;
            } else if metadata.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| Error::Internal("invalid local skill root".into()))?
                    .to_str()
                    .ok_or_else(|| Error::Conflict("local skill: require UTF-8 paths".into()))?
                    .to_owned();
                manifest::relative_path(&relative)?;
                tree.insert(
                    relative,
                    FileContent {
                        bytes: Arc::from(content::read_file(&path, content::MAX_FILE)?),
                        executable: content::executable(&metadata),
                    },
                );
                if tree.byte_len() > content::MAX_TREE {
                    return Err(Error::Conflict(
                        "local skill: content exceeds 128 MiB".into(),
                    ));
                }
            } else {
                return Err(Error::Conflict(
                    "local skill: symlinks and special files are forbidden".into(),
                ));
            }
        }
        Ok(())
    }
    let mut tree = Tree::new();
    let path = root.join(relative);
    visit(&path, &path, &mut tree)?;
    tree.validate()?;
    let body = tree
        .get("SKILL.md")
        .ok_or_else(|| Error::Conflict(format!("{name}: local skill lacks SKILL.md")))?;
    validate_frontmatter(&body.bytes, name)?;
    Ok(tree)
}

fn add_file(
    root: &Path,
    path: &str,
    bytes: Vec<u8>,
    operations: &mut Vec<Operation>,
) -> Result<()> {
    let before = content::fingerprint(root, path)?;
    if before
        .as_ref()
        .is_some_and(|fingerprint| fingerprint.directory)
    {
        return Err(Error::Conflict(format!(
            "{path}: require a regular metadata file"
        )));
    }
    let after = Some(Fingerprint {
        hash: blake3::hash(&bytes).to_hex().to_string(),
        directory: false,
    });
    if before != after {
        operations.push(Operation {
            path: path.into(),
            before,
            after,
            payload: Payload::File(Arc::from(bytes)),
        });
    }
    Ok(())
}

fn ignore_content<'a>(root: &Path, outputs: impl Iterator<Item = &'a String>) -> Result<String> {
    let bytes =
        content::read_project_file(root, ".gitignore", content::MAX_FILE)?.unwrap_or_default();
    let existing = String::from_utf8(bytes)
        .map_err(|_| Error::Conflict(".gitignore: require UTF-8 text".into()))?;
    let mut start = None;
    let mut end = None;
    let mut offset = 0;
    for line in existing.split_inclusive('\n') {
        let text = line.trim_end_matches(['\r', '\n']);
        if text == BEGIN {
            if start.is_some() || end.is_some() {
                return Err(Error::Conflict(
                    ".gitignore: duplicate managed block".into(),
                ));
            }
            start = Some(offset);
        }
        if text == END {
            if start.is_none() || end.is_some() {
                return Err(Error::Conflict(".gitignore: invalid managed block".into()));
            }
            end = Some(offset + line.len());
        }
        offset += line.len();
    }
    if start.is_some() != end.is_some() {
        return Err(Error::Conflict(
            ".gitignore: incomplete managed block".into(),
        ));
    }
    let (prefix, suffix) = match (start, end) {
        (Some(start), Some(end)) => (&existing[..start], &existing[end..]),
        _ => (existing.as_str(), ""),
    };
    let mut result = prefix.to_owned();
    if !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result.push_str(BEGIN);
    result.push('\n');
    for path in [STATE_PATH, LOCK_PATH, TRANSACTION_PATH] {
        result.push('/');
        result.push_str(path);
        result.push('\n');
    }
    result.push_str("/.agents/.skills-stage-*\n");
    for path in outputs {
        result.push('/');
        result.push_str(path);
        result.push_str("/\n");
    }
    result.push_str(END);
    result.push('\n');
    result.push_str(suffix);
    Ok(result)
}
