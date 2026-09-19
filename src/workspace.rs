use crate::error::{Error, IoContext, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// The ordered project roots one command applies to.
///
/// A single root preserves the original one-project behaviour.
/// Several roots fan one manifest across the checked-out worktrees of one repository.
/// Harnesses stop their skill search at the worktree they start in.
/// A container directory therefore cannot hold the projection for its lanes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    roots: Vec<PathBuf>,
}

impl Workspace {
    /// Select the given directory alone.
    pub fn single(root: PathBuf) -> Self {
        Self { roots: vec![root] }
    }

    /// Select every checked-out worktree of the repository at `root`.
    ///
    /// The bare entry of a container carries no working tree.
    /// This constructor drops it, and drops any registered path that is gone.
    pub fn worktrees(root: &Path) -> Result<Self> {
        let output = Command::new("git")
            .current_dir(root)
            .args(["worktree", "list", "--porcelain"])
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .context("list repository worktrees")?;
        if !output.status.success() {
            return Err(Error::Invalid(
                "project: Git cannot list worktrees; require a repository".into(),
            ));
        }
        let listing = std::str::from_utf8(&output.stdout)
            .map_err(|_| Error::Conflict("project: require UTF-8 worktree paths".into()))?;
        let mut roots = Vec::new();
        for record in parse_worktree_records(listing) {
            let path = Path::new(&record);
            if !path.is_dir() {
                continue;
            }
            roots.push(std::fs::canonicalize(path).context("resolve worktree directory")?);
        }
        roots.sort();
        roots.dedup();
        if roots.is_empty() {
            return Err(Error::Invalid(
                "project: the repository has no checked-out worktree".into(),
            ));
        }
        Ok(Self { roots })
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Report whether the command applies to more than one project root.
    pub fn is_fanned_out(&self) -> bool {
        self.roots.len() > 1
    }
}

/// Collect the working-tree paths of a `git worktree list --porcelain` listing.
///
/// A record starts with a `worktree` attribute and ends at a blank line.
/// A `bare` attribute marks a record without a working tree.
fn parse_worktree_records(listing: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut current: Option<String> = None;
    for line in listing.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current = Some(path.to_owned());
        } else if line == "bare" {
            current = None;
        } else if line.is_empty()
            && let Some(path) = current.take()
        {
            paths.push(path);
        }
    }
    if let Some(path) = current {
        paths.push(path);
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_the_bare_record_of_a_container() {
        let listing = "worktree /repo/.bare\nbare\n\n\
                       worktree /repo/develop\nHEAD abc\nbranch refs/heads/develop\n\n\
                       worktree /repo/420\nHEAD def\nbranch refs/heads/feature/420\n\n";
        assert_eq!(
            parse_worktree_records(listing),
            vec!["/repo/develop".to_owned(), "/repo/420".to_owned()]
        );
    }

    #[test]
    fn keeps_a_detached_worktree_and_a_trailing_record() {
        let listing = "worktree /repo/main\nHEAD abc\nbranch refs/heads/main\n\n\
                       worktree /repo/review\nHEAD def\ndetached\n";
        assert_eq!(
            parse_worktree_records(listing),
            vec!["/repo/main".to_owned(), "/repo/review".to_owned()]
        );
    }

    #[test]
    fn reads_an_ordinary_repository_as_one_root() {
        let listing = "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n";
        assert_eq!(parse_worktree_records(listing), vec!["/repo".to_owned()]);
    }

    #[test]
    fn single_selects_one_root() {
        let workspace = Workspace::single(PathBuf::from("/repo/develop"));
        assert_eq!(workspace.roots(), [PathBuf::from("/repo/develop")]);
        assert!(!workspace.is_fanned_out());
    }
}
