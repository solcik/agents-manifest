use crate::{
    content::{self, Fingerprint},
    error::{Error, IoContext, Result},
    plan::{self, LOCK_PATH, Operation, Payload, Plan, TRANSACTION_PATH},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub struct ProjectLock {
    root: PathBuf,
    _file: fs::File,
}

pub struct ProjectReadLock {
    _file: fs::File,
}

impl ProjectLock {
    pub fn acquire(root: &Path) -> Result<Self> {
        content::safe_ancestors(root, LOCK_PATH)?;
        plan::validate_operation_ownership(root, std::iter::empty())?;
        fs::create_dir_all(root.join(".agents")).context("create project metadata directory")?;
        content::sync_dir(root)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join(LOCK_PATH))
            .context("open project lock")?;
        file.try_lock().map_err(|_| {
            Error::Conflict("project: another process holds the synchronisation lock".into())
        })?;
        Ok(Self {
            root: root.to_owned(),
            _file: file,
        })
    }
}

impl ProjectReadLock {
    pub fn acquire(root: &Path) -> Result<Option<Self>> {
        content::safe_ancestors(root, LOCK_PATH)?;
        match fs::File::open(root.join(LOCK_PATH)) {
            Ok(file) => {
                file.try_lock_shared().map_err(|_| {
                    Error::Conflict("project: synchronisation is in progress".into())
                })?;
                Ok(Some(Self { _file: file }))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(Error::Io {
                operation: "open project read lock",
                source,
            }),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalEntry {
    path: String,
    before: Option<Fingerprint>,
    after: Option<Fingerprint>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    entries: Vec<JournalEntry>,
}

pub struct Transaction<'a> {
    root: &'a Path,
    _lock: &'a ProjectLock,
}

impl<'a> Transaction<'a> {
    pub fn new(lock: &'a ProjectLock) -> Self {
        Self {
            root: &lock.root,
            _lock: lock,
        }
    }

    pub fn recover(&self) -> Result<bool> {
        content::safe_ancestors(self.root, TRANSACTION_PATH)?;
        let directory = self.root.join(TRANSACTION_PATH);
        if !directory.exists() {
            return Ok(false);
        }
        let journal_path = directory.join("journal.json");
        content::safe_ancestors(&directory, "journal.json")?;
        let bytes = content::read_file(&journal_path, content::MAX_FILE)?;
        if bytes.len() > content::MAX_FILE {
            return Err(Error::Conflict(
                "transaction: oversized publication journal".into(),
            ));
        }
        let journal: Journal = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Conflict("transaction: invalid publication journal".into()))?;
        self.validate_journal(&journal)?;
        plan::validate_operation_ownership(
            self.root,
            journal.entries.iter().map(|entry| entry.path.as_str()),
        )?;
        content::safe_ancestors(&directory, "committed")?;
        if !directory.join("committed").exists() {
            self.rollback(&directory, &journal)?;
        }
        self.cleanup(&directory)?;
        Ok(true)
    }

    pub fn apply(&self, plan: &Plan) -> Result<()> {
        if plan.operations.is_empty() {
            return Ok(());
        }
        plan::validate_operation_ownership(
            self.root,
            plan.operations
                .iter()
                .map(|operation| operation.path.as_str()),
        )?;
        if self.root.join(TRANSACTION_PATH).exists() {
            return Err(Error::Conflict(
                "transaction: recover the previous publication first".into(),
            ));
        }
        let temporary = tempfile::Builder::new()
            .prefix(".skills-stage-")
            .tempdir_in(self.root.join(".agents"))
            .context("create publication staging directory")?;
        fs::create_dir(temporary.path().join("new")).context("create staged output directory")?;
        fs::create_dir(temporary.path().join("old"))
            .context("create publication backup directory")?;
        let journal = Journal {
            version: 1,
            entries: plan
                .operations
                .iter()
                .map(|op| JournalEntry {
                    path: op.path.clone(),
                    before: op.before.clone(),
                    after: op.after.clone(),
                })
                .collect(),
        };
        self.validate_journal(&journal)?;
        for (index, operation) in plan.operations.iter().enumerate() {
            let staged = temporary.path().join("new").join(index.to_string());
            match &operation.payload {
                Payload::Tree(tree) => tree.write(&staged)?,
                Payload::File(bytes) => {
                    let mut file =
                        fs::File::create_new(&staged).context("create staged metadata")?;
                    file.write_all(bytes).context("write staged metadata")?;
                    file.sync_all().context("flush staged metadata")?;
                }
                Payload::Remove => {}
            }
            if content::fingerprint_path(&staged)? != operation.after {
                return Err(Error::Internal(
                    "staged content does not match the plan".into(),
                ));
            }
        }
        let bytes = serde_json::to_vec(&journal)
            .map_err(|_| Error::Internal("cannot encode publication journal".into()))?;
        let mut file = fs::File::create_new(temporary.path().join("journal.json"))
            .context("create publication journal")?;
        file.write_all(&bytes)
            .context("write publication journal")?;
        file.sync_all().context("flush publication journal")?;
        content::sync_dir(&temporary.path().join("new"))?;
        content::sync_dir(&temporary.path().join("old"))?;
        content::sync_dir(temporary.path())?;
        // Inspect every destination again before the first project replacement.
        for operation in &plan.operations {
            if content::fingerprint(self.root, &operation.path)? != operation.before {
                return Err(Error::Conflict(format!(
                    "{}: project changed after planning",
                    operation.path
                )));
            }
        }
        let directory = self.root.join(TRANSACTION_PATH);
        fs::rename(temporary.path(), &directory).context("publish transaction journal")?;
        content::sync_dir(&self.root.join(".agents"))?;
        let result = plan
            .operations
            .iter()
            .enumerate()
            .try_for_each(|(index, op)| self.replace(&directory, index, op));
        if let Err(error) = result {
            if let Err(recovery) = self.rollback(&directory, &journal) {
                return Err(Error::Conflict(format!(
                    "publication failed: {error}; recovery requires attention: {recovery}"
                )));
            }
            self.cleanup(&directory)?;
            return Err(error);
        }
        let marker =
            fs::File::create_new(directory.join("committed")).context("commit publication")?;
        marker.sync_all().context("flush publication commit")?;
        content::sync_dir(&directory)?;
        self.cleanup(&directory)
    }

    fn replace(&self, directory: &Path, index: usize, operation: &Operation) -> Result<()> {
        content::safe_ancestors(self.root, &operation.path)?;
        let target = self.root.join(&operation.path);
        let parent = target
            .parent()
            .ok_or_else(|| Error::Internal("missing output parent".into()))?;
        fs::create_dir_all(parent).context("create output parent")?;
        let mut ancestor = Some(parent);
        while let Some(directory) = ancestor {
            content::sync_dir(directory)?;
            if directory == self.root {
                break;
            }
            ancestor = directory.parent();
        }
        if content::fingerprint(self.root, &operation.path)? != operation.before {
            return Err(Error::Conflict(format!(
                "{}: project changed during publication",
                operation.path
            )));
        }
        if operation.before.is_some() {
            let backup = directory.join("old").join(index.to_string());
            fs::rename(&target, &backup).context("preserve previous output")?;
            content::sync_dir(parent)?;
            content::sync_dir(&directory.join("old"))?;
            if content::fingerprint_path(&backup)? != operation.before {
                return Err(Error::Conflict(format!(
                    "{}: previous output changed during replacement",
                    operation.path
                )));
            }
        }
        if operation.after.is_some() {
            fs::rename(directory.join("new").join(index.to_string()), &target)
                .context("publish planned output")?;
            content::sync_dir(parent)?;
            content::sync_dir(&directory.join("new"))?;
        }
        Ok(())
    }

    fn validate_journal(&self, journal: &Journal) -> Result<()> {
        let mut paths = std::collections::BTreeSet::new();
        if journal.version != 1 || journal.entries.len() > content::MAX_FILES {
            return Err(Error::Conflict(
                "transaction: unsupported journal version or entry count".into(),
            ));
        }
        for entry in &journal.entries {
            if !plan::operation_path(&entry.path)
                || !paths.insert(&entry.path)
                || entry
                    .before
                    .iter()
                    .chain(entry.after.iter())
                    .any(|fp| !plan::valid_hash(&fp.hash))
                || entry.before.is_none() && entry.after.is_none()
                || entry
                    .before
                    .iter()
                    .chain(entry.after.iter())
                    .any(|fp| fp.directory != plan::output_path(&entry.path))
            {
                return Err(Error::Conflict(
                    "transaction: invalid publication entry".into(),
                ));
            }
        }
        Ok(())
    }

    fn rollback(&self, directory: &Path, journal: &Journal) -> Result<()> {
        content::safe_ancestors(directory, "old")?;
        for (index, entry) in journal.entries.iter().enumerate().rev() {
            let backup_relative = format!("old/{index}");
            content::safe_ancestors(directory, &backup_relative)?;
            let backup = directory.join(backup_relative);
            let saved = content::fingerprint_path(&backup)?;
            let current = content::fingerprint(self.root, &entry.path)?;
            let target = self.root.join(&entry.path);
            if let Some(saved) = saved {
                if Some(saved) != entry.before {
                    return Err(Error::Conflict(format!(
                        "{}: backup changed; preserved for manual recovery",
                        entry.path
                    )));
                }
                if current.is_some() && current != entry.after {
                    return Err(Error::Conflict(format!(
                        "{}: output changed; preserved with its backup",
                        entry.path
                    )));
                }
                if current.is_some() {
                    remove_owned(&target, current.as_ref())?;
                }
                fs::rename(backup, &target).context("restore previous output")?;
                content::sync_dir(
                    target
                        .parent()
                        .ok_or_else(|| Error::Internal("missing recovery parent".into()))?,
                )?;
                content::sync_dir(&directory.join("old"))?;
            } else if entry.before.is_none() {
                if current.is_some() {
                    if current != entry.after {
                        return Err(Error::Conflict(format!(
                            "{}: new output changed; preserved",
                            entry.path
                        )));
                    }
                    remove_owned(&target, current.as_ref())?;
                    content::sync_dir(
                        target
                            .parent()
                            .ok_or_else(|| Error::Internal("missing recovery parent".into()))?,
                    )?;
                }
            } else if current != entry.before {
                return Err(Error::Conflict(format!(
                    "{}: previous output is missing; recovery stopped",
                    entry.path
                )));
            }
        }
        Ok(())
    }

    fn cleanup(&self, directory: &Path) -> Result<()> {
        // Retire the journal atomically before cleanup. A cleanup crash cannot block recovery.
        let retired = tempfile::Builder::new()
            .prefix(".skills-stage-retired-")
            .tempdir_in(self.root.join(".agents"))
            .context("reserve transaction cleanup directory")?;
        fs::rename(directory, retired.path()).context("retire completed transaction")?;
        content::sync_dir(&self.root.join(".agents"))?;
        retired.close().context("remove completed transaction data")
    }
}

fn remove_owned(path: &Path, fingerprint: Option<&Fingerprint>) -> Result<()> {
    if fingerprint.is_some_and(|fingerprint| fingerprint.directory) {
        fs::remove_dir_all(path).context("remove recorded generated directory")
    } else {
        fs::remove_file(path).context("remove recorded generated file")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn fingerprint(bytes: &[u8]) -> Fingerprint {
        Fingerprint {
            hash: blake3::hash(bytes).to_hex().to_string(),
            directory: false,
        }
    }

    fn interrupted(phase: usize, committed: bool) -> (tempfile::TempDir, ProjectLock) {
        let project = tempfile::tempdir().unwrap();
        assert!(
            std::process::Command::new("git")
                .current_dir(project.path())
                .args(["init", "--quiet"])
                .status()
                .unwrap()
                .success()
        );
        let lock = ProjectLock::acquire(project.path()).unwrap();
        let directory = project.path().join(TRANSACTION_PATH);
        fs::create_dir(&directory).unwrap();
        fs::create_dir(directory.join("old")).unwrap();
        fs::create_dir(directory.join("new")).unwrap();
        fs::write(project.path().join(".gitignore"), b"old ignore\n").unwrap();
        let journal = Journal {
            version: 1,
            entries: vec![
                JournalEntry {
                    path: ".gitignore".into(),
                    before: Some(fingerprint(b"old ignore\n")),
                    after: Some(fingerprint(b"new ignore\n")),
                },
                JournalEntry {
                    path: plan::STATE_PATH.into(),
                    before: None,
                    after: Some(fingerprint(b"new state\n")),
                },
            ],
        };
        fs::write(
            directory.join("journal.json"),
            serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
        fs::write(directory.join("new/0"), b"new ignore\n").unwrap();
        fs::write(directory.join("new/1"), b"new state\n").unwrap();
        if phase >= 1 {
            fs::rename(project.path().join(".gitignore"), directory.join("old/0")).unwrap();
        }
        if phase >= 2 {
            fs::rename(directory.join("new/0"), project.path().join(".gitignore")).unwrap();
        }
        if phase >= 3 {
            fs::rename(
                directory.join("new/1"),
                project.path().join(plan::STATE_PATH),
            )
            .unwrap();
        }
        if committed {
            fs::write(directory.join("committed"), b"").unwrap();
        }
        (project, lock)
    }

    #[test]
    fn recovers_every_uncommitted_rename_boundary_and_is_idempotent() {
        for phase in 0..=3 {
            let (project, lock) = interrupted(phase, false);
            let transaction = Transaction::new(&lock);
            assert!(transaction.recover().unwrap());
            assert_eq!(
                fs::read(project.path().join(".gitignore")).unwrap(),
                b"old ignore\n"
            );
            assert!(!project.path().join(plan::STATE_PATH).exists());
            assert!(!project.path().join(TRANSACTION_PATH).exists());
            assert!(!transaction.recover().unwrap());
        }
    }

    #[test]
    fn committed_recovery_keeps_published_content() {
        let (project, lock) = interrupted(3, true);
        Transaction::new(&lock).recover().unwrap();
        assert_eq!(
            fs::read(project.path().join(".gitignore")).unwrap(),
            b"new ignore\n"
        );
        assert_eq!(
            fs::read(project.path().join(plan::STATE_PATH)).unwrap(),
            b"new state\n"
        );
    }

    #[test]
    fn recovery_preserves_new_user_edits_and_their_backups() {
        let (project, lock) = interrupted(2, false);
        fs::write(project.path().join(".gitignore"), b"user edit\n").unwrap();
        assert!(matches!(
            Transaction::new(&lock).recover(),
            Err(Error::Conflict(_))
        ));
        assert_eq!(
            fs::read(project.path().join(".gitignore")).unwrap(),
            b"user edit\n"
        );
        assert_eq!(
            fs::read(project.path().join(TRANSACTION_PATH).join("old/0")).unwrap(),
            b"old ignore\n"
        );
    }

    #[test]
    fn invalid_journal_paths_cannot_escape_project() {
        let (project, lock) = interrupted(0, false);
        let journal = Journal {
            version: 1,
            entries: vec![JournalEntry {
                path: "../outside".into(),
                before: None,
                after: Some(fingerprint(b"x")),
            }],
        };
        fs::write(
            project.path().join(TRANSACTION_PATH).join("journal.json"),
            serde_json::to_vec(&journal).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            Transaction::new(&lock).recover(),
            Err(Error::Conflict(_))
        ));
        assert_eq!(
            fs::read(project.path().join(".gitignore")).unwrap(),
            b"old ignore\n"
        );
    }
}
