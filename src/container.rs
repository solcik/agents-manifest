use crate::{
    error::{Error, IoContext, Result},
    plan::Action,
    workspace::{WorktreeRecord, parse_worktree_list},
};
use serde::Serialize;
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    process::Command,
};

/// The skill discovery directories that a container root links.
///
/// Codex, Pi and OpenCode read `.agents/skills`. Claude reads `.claude/skills`.
pub const SKILL_DIRECTORIES: [&str; 2] = [".agents/skills", ".claude/skills"];

/// A worktree container and the worktree that its root follows.
///
/// A container holds a bare `.bare` Git directory and one worktree per branch.
/// A harness that starts in the container root finds no worktree there.
/// The root therefore links the skill directories of one base worktree.
/// A sync in the base worktree then updates the root without a second projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Container {
    root: PathBuf,
    base: PathBuf,
}

/// One symlink from the container root into the base worktree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Link {
    /// The link path relative to the container root.
    pub path: String,
    /// The relative link target, as the symlink stores it.
    pub target: PathBuf,
}

/// What the container root holds at a link path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Observed {
    Missing,
    Symlink(PathBuf),
    Other,
}

#[derive(Clone, Debug, Serialize)]
pub struct LinkChange {
    pub action: Action,
    pub path: String,
    pub target: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct LinkReport {
    pub version: u32,
    pub container: PathBuf,
    pub base: PathBuf,
    pub changes: Vec<LinkChange>,
}

impl Container {
    /// Resolve the container of `project` and its base worktree.
    ///
    /// `project` can be the container root or any of its worktrees.
    /// `base` overrides the base worktree. Without it, the base is the worktree
    /// that holds the remote default branch, or else the branch of the bare HEAD.
    pub fn resolve(project: &Path, base: Option<&Path>) -> Result<Self> {
        let common = git(
            project,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .map_err(|_| Error::Invalid("project: Git cannot find a repository".into()))?;
        let common = PathBuf::from(common);
        if common.file_name().and_then(|name| name.to_str()) != Some(".bare") {
            return Err(Error::Invalid(
                "project: require a worktree container with a .bare Git directory".into(),
            ));
        }
        let root = common
            .parent()
            .ok_or_else(|| Error::Invalid("project: the .bare directory has no parent".into()))?;
        let root = fs::canonicalize(root).context("resolve container directory")?;
        let listing = git(&common, &["worktree", "list", "--porcelain"])
            .map_err(|_| Error::Invalid("project: Git cannot list worktrees".into()))?;
        let records: Vec<WorktreeRecord> = parse_worktree_list(&listing)
            .into_iter()
            .filter(|record| Path::new(&record.path).is_dir())
            .collect();
        let base = match base {
            Some(path) => {
                let wanted = fs::canonicalize(path).context("resolve base worktree")?;
                records
                    .iter()
                    .find(|record| same_directory(&record.path, &wanted))
                    .ok_or_else(|| {
                        Error::Invalid(format!(
                            "base: {} is not a worktree of this container",
                            path.display()
                        ))
                    })?;
                wanted
            }
            None => {
                let branch = base_branch(&common)?;
                let record = records
                    .iter()
                    .find(|record| record.branch.as_deref() == Some(branch.as_str()))
                    .ok_or_else(|| {
                        Error::Invalid(format!(
                            "base: no worktree has branch {branch} checked out; add one or pass --base"
                        ))
                    })?;
                fs::canonicalize(&record.path).context("resolve base worktree")?
            }
        };
        if base.strip_prefix(&root).is_err() {
            return Err(Error::Invalid(
                "base: the base worktree lies outside the container".into(),
            ));
        }
        Ok(Self { root, base })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    /// The links for every skill directory that the base worktree contains.
    pub fn links(&self) -> Result<Vec<Link>> {
        let lane = self
            .base
            .strip_prefix(&self.root)
            .map_err(|_| Error::Internal("base worktree outside container".into()))?;
        let links: Vec<Link> = SKILL_DIRECTORIES
            .iter()
            .filter(|directory| self.base.join(directory).is_dir())
            .map(|directory| Link {
                path: (*directory).to_owned(),
                target: relative_target(Path::new(directory), lane),
            })
            .collect();
        if links.is_empty() {
            return Err(Error::Invalid(format!(
                "base: {} has no skill directory; run agent-skills sync there first",
                self.base.display()
            )));
        }
        Ok(links)
    }

    /// Observe what the container root holds at one link path.
    pub fn observe(&self, link: &Link) -> Result<Observed> {
        let path = self.root.join(&link.path);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Observed::Missing),
            Err(error) => Err(Error::Io {
                operation: "inspect container link",
                source: error,
            }),
            Ok(metadata) if metadata.file_type().is_symlink() => Ok(Observed::Symlink(
                fs::read_link(&path).context("read container link")?,
            )),
            Ok(_) => Ok(Observed::Other),
        }
    }

    /// Point one link path at its target.
    ///
    /// A new symlink replaces the old one through a rename, so no reader sees a gap.
    pub fn apply(&self, link: &Link) -> Result<()> {
        let path = self.root.join(&link.path);
        let parent = path
            .parent()
            .ok_or_else(|| Error::Internal("link path has no parent".into()))?;
        fs::create_dir_all(parent).context("create container link directory")?;
        let staged = parent.join(".skills-link-stage");
        match fs::remove_file(&staged) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                return Err(Error::Io {
                    operation: "remove staged container link",
                    source: error,
                });
            }
            _ => {}
        }
        std::os::unix::fs::symlink(&link.target, &staged).context("stage container link")?;
        fs::rename(&staged, &path).context("publish container link")
    }
}

/// Decide the action for one link from its observed state.
///
/// A real file or directory at the link path is never replaced.
/// It can hold a copied projection or user content.
pub fn reconcile(link: &Link, observed: &Observed) -> Result<Option<Action>> {
    match observed {
        Observed::Missing => Ok(Some(Action::Create)),
        Observed::Symlink(target) if *target == link.target => Ok(None),
        Observed::Symlink(_) => Ok(Some(Action::Update)),
        Observed::Other => Err(Error::Conflict(format!(
            "{}: the container root holds a real directory here; remove it, then run link again",
            link.path
        ))),
    }
}

/// The target of a link at `directory` (relative to the container root)
/// that points at the same directory inside the lane `lane`.
///
/// `.agents/skills` in lane `main` gives `../main/.agents/skills`.
/// A relative target stays valid wherever the container is mounted.
pub fn relative_target(directory: &Path, lane: &Path) -> PathBuf {
    let depth = directory
        .parent()
        .map(|parent| {
            parent
                .components()
                .filter(|component| matches!(component, Component::Normal(_)))
                .count()
        })
        .unwrap_or(0);
    let mut target = PathBuf::new();
    for _ in 0..depth {
        target.push("..");
    }
    target.push(lane);
    target.push(directory);
    target
}

/// The branch that the base worktree holds.
fn base_branch(common: &Path) -> Result<String> {
    if let Ok(remote) = git(
        common,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) && let Some(branch) = remote.strip_prefix("origin/")
    {
        return Ok(branch.to_owned());
    }
    git(common, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .map_err(|_| Error::Invalid("base: cannot find the default branch; pass --base".into()))
}

fn same_directory(recorded: &str, wanted: &Path) -> bool {
    fs::canonicalize(recorded).is_ok_and(|path| path == wanted)
}

fn git(directory: &Path, args: &[&str]) -> std::result::Result<String, ()> {
    let output = Command::new("git")
        .current_dir(directory)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .map_err(|_| ())?;
    if !output.status.success() {
        return Err(());
    }
    String::from_utf8(output.stdout)
        .map(|text| text.trim_end().to_owned())
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn link() -> Link {
        Link {
            path: ".agents/skills".into(),
            target: PathBuf::from("../main/.agents/skills"),
        }
    }

    #[test]
    fn a_target_climbs_out_of_the_link_parent_into_the_lane() {
        assert_eq!(
            relative_target(Path::new(".agents/skills"), Path::new("main")),
            PathBuf::from("../main/.agents/skills")
        );
        assert_eq!(
            relative_target(Path::new(".claude/skills"), Path::new("develop")),
            PathBuf::from("../develop/.claude/skills")
        );
    }

    #[test]
    fn a_missing_link_is_created_and_a_matching_one_is_kept() {
        assert_eq!(
            reconcile(&link(), &Observed::Missing).unwrap(),
            Some(Action::Create)
        );
        assert_eq!(
            reconcile(&link(), &Observed::Symlink("../main/.agents/skills".into())).unwrap(),
            None
        );
    }

    #[test]
    fn a_link_to_another_lane_is_updated() {
        assert_eq!(
            reconcile(&link(), &Observed::Symlink("../old/.agents/skills".into())).unwrap(),
            Some(Action::Update)
        );
    }

    #[test]
    fn a_real_directory_is_never_replaced() {
        let error = reconcile(&link(), &Observed::Other).unwrap_err();
        assert_eq!(error.code(), 4);
    }
}
