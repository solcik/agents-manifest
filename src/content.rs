use crate::{
    error::{Error, IoContext, Result},
    manifest::relative_path,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Read, path::Path, sync::Arc};
use unicode_normalization::UnicodeNormalization;

pub const MAX_FILE: usize = 32 * 1024 * 1024;
pub const MAX_TREE: usize = 128 * 1024 * 1024;
pub const MAX_FILES: usize = 10_000;

pub fn read_file(path: &Path, limit: usize) -> Result<Vec<u8>> {
    if path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(manifest_secret_name)
    }) {
        return Err(Error::Conflict(
            "file: secret paths cannot become skill content".into(),
        ));
    }
    let metadata = fs::symlink_metadata(path).context("inspect regular file")?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(Error::Conflict(
            "file: require a regular file within the size limit".into(),
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .context("open regular file")?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .context("read bounded file")?;
    if bytes.len() > limit {
        return Err(Error::Conflict(
            "file: content grew beyond the size limit".into(),
        ));
    }
    Ok(bytes)
}

fn manifest_secret_name(name: &str) -> bool {
    name == ".env.local"
        || name.starts_with(".env.") && name.ends_with(".local")
        || name == ".sops-age-key.txt"
}

pub fn read_project_file(root: &Path, relative: &str, limit: usize) -> Result<Option<Vec<u8>>> {
    safe_ancestors(root, relative)?;
    let path = root.join(relative);
    match fs::symlink_metadata(&path) {
        Ok(_) => read_file(&path, limit).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io {
            operation: "inspect project file",
            source,
        }),
    }
}

#[derive(Clone, Debug)]
pub struct FileContent {
    pub bytes: Arc<[u8]>,
    pub executable: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Tree {
    files: BTreeMap<String, FileContent>,
    byte_len: usize,
}

impl Tree {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn get(&self, path: &str) -> Option<&FileContent> {
        self.files.get(path)
    }
    pub fn insert(&mut self, path: String, file: FileContent) -> Option<FileContent> {
        self.byte_len += file.bytes.len();
        let previous = self.files.insert(path, file);
        if let Some(previous) = &previous {
            self.byte_len -= previous.bytes.len();
        }
        previous
    }
    pub fn values(&self) -> impl Iterator<Item = &FileContent> {
        self.files.values()
    }
    pub fn byte_len(&self) -> usize {
        self.byte_len
    }

    pub fn validate(&self) -> Result<()> {
        if self.byte_len > MAX_TREE || self.files.len() > MAX_FILES {
            return Err(Error::Source(
                "skill: content exceeds the tree size or entry limit".into(),
            ));
        }
        let mut paths = BTreeMap::<String, (String, bool)>::new();
        for (path, file) in self {
            relative_path(path)?;
            if file.bytes.len() > MAX_FILE {
                return Err(Error::Source("skill: file exceeds 32 MiB".into()));
            }
            let mut candidate = Some((Path::new(path), false));
            while let Some((entry, directory)) = candidate {
                let entry = entry
                    .to_str()
                    .ok_or_else(|| Error::Source("skill: require UTF-8 paths".into()))?;
                if entry.is_empty() {
                    break;
                }
                let key: String = entry.nfd().flat_map(char::to_lowercase).collect();
                if let Some((previous, previous_directory)) = paths.get(&key) {
                    if previous != entry || *previous_directory != directory {
                        return Err(Error::Source(
                            "skill: paths collide on a case-insensitive filesystem".into(),
                        ));
                    }
                } else {
                    paths.insert(key, (entry.to_owned(), directory));
                }
                candidate = Path::new(entry).parent().map(|parent| (parent, true));
            }
        }
        if paths.len() > MAX_FILES {
            return Err(Error::Source(
                "skill: implied directories exceed the entry limit".into(),
            ));
        }
        Ok(())
    }
    pub fn fingerprint(&self) -> Fingerprint {
        let mut entries = BTreeMap::new();
        for (path, content) in self {
            let mut parent = Path::new(path).parent();
            while let Some(directory) = parent {
                if directory.as_os_str().is_empty() {
                    break;
                }
                entries.insert(
                    directory.to_string_lossy().into_owned(),
                    EntryFingerprint::directory(),
                );
                parent = directory.parent();
            }
            entries.insert(
                path.clone(),
                EntryFingerprint::file(
                    blake3::hash(&content.bytes).to_hex().to_string(),
                    content.executable,
                ),
            );
        }
        Fingerprint {
            hash: hash_entries(&entries),
            directory: true,
        }
    }
    pub fn write(&self, path: &Path) -> Result<()> {
        self.validate()?;
        fs::create_dir(path).context("create staged tree")?;
        for (relative, content) in self {
            relative_path(relative)?;
            let target = path.join(relative);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).context("create staged directory")?;
            }
            let mut file = fs::File::create_new(&target).context("create staged file")?;
            use std::io::Write;
            file.write_all(&content.bytes)
                .context("write staged file")?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(fs::Permissions::from_mode(if content.executable {
                    0o755
                } else {
                    0o644
                }))
                .context("set staged file permissions")?;
            }
            file.sync_all().context("flush staged file")?;
        }
        sync_tree_dirs(path)?;
        Ok(())
    }
}

impl<'a> IntoIterator for &'a Tree {
    type Item = (&'a String, &'a FileContent);
    type IntoIter = std::collections::btree_map::Iter<'a, String, FileContent>;
    fn into_iter(self) -> Self::IntoIter {
        self.files.iter()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Fingerprint {
    pub hash: String,
    pub directory: bool,
}

struct EntryFingerprint {
    directory: bool,
    executable: bool,
    hash: String,
}

impl EntryFingerprint {
    fn directory() -> Self {
        Self {
            directory: true,
            executable: false,
            hash: String::new(),
        }
    }
    fn file(hash: String, executable: bool) -> Self {
        Self {
            directory: false,
            executable,
            hash,
        }
    }
}

fn hash_entries(entries: &BTreeMap<String, EntryFingerprint>) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"agents-manifest-tree-v1\0");
    for (
        path,
        EntryFingerprint {
            directory,
            executable,
            hash,
        },
    ) in entries
    {
        hasher.update(&(path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update(&[u8::from(*directory), u8::from(*executable)]);
        hasher.update(hash.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

pub fn safe_ancestors(root: &Path, relative: &str) -> Result<()> {
    relative_path(relative)?;
    let mut current = root.to_owned();
    for component in Path::new(relative).components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::Conflict(format!(
                    "{relative}: symlinks are not managed output"
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect project ancestors",
                    source,
                });
            }
        }
    }
    Ok(())
}

pub fn fingerprint(root: &Path, relative: &str) -> Result<Option<Fingerprint>> {
    safe_ancestors(root, relative)?;
    fingerprint_path(&root.join(relative))
}

pub fn fingerprint_path(path: &Path) -> Result<Option<Fingerprint>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect managed output",
                source,
            });
        }
    };
    if metadata.is_file() {
        return Ok(Some(Fingerprint {
            hash: hash_file(path, metadata.len())?,
            directory: false,
        }));
    }
    if !metadata.is_dir() {
        return Err(Error::Conflict(
            "managed output: require regular files and directories".into(),
        ));
    }
    let mut entries = BTreeMap::new();
    let mut total = 0u64;
    inspect_tree(path, path, &mut entries, &mut total)?;
    Ok(Some(Fingerprint {
        hash: hash_entries(&entries),
        directory: true,
    }))
}

fn inspect_tree(
    root: &Path,
    directory: &Path,
    entries: &mut BTreeMap<String, EntryFingerprint>,
    total: &mut u64,
) -> Result<()> {
    for entry in fs::read_dir(directory).context("read generated directory")? {
        let entry = entry.context("read generated entry")?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| Error::Internal("invalid tree root".into()))?
            .to_str()
            .ok_or_else(|| Error::Conflict("generated output: require UTF-8 paths".into()))?
            .to_owned();
        relative_path(&relative)
            .map_err(|_| Error::Conflict("generated output: unsafe entry path".into()))?;
        let metadata = fs::symlink_metadata(&path).context("inspect generated entry")?;
        if metadata.is_dir() {
            entries.insert(relative, EntryFingerprint::directory());
            if entries.len() > MAX_FILES {
                return Err(Error::Conflict("generated output: too many entries".into()));
            }
            inspect_tree(root, &path, entries, total)?;
        } else if metadata.is_file() {
            *total += metadata.len();
            if *total > MAX_TREE as u64 {
                return Err(Error::Conflict(
                    "generated output: tree exceeds 128 MiB".into(),
                ));
            }
            entries.insert(
                relative,
                EntryFingerprint::file(hash_file(&path, metadata.len())?, executable(&metadata)),
            );
        } else {
            return Err(Error::Conflict(
                "generated output: symlinks and special files are forbidden".into(),
            ));
        }
        if entries.len() > MAX_FILES {
            return Err(Error::Conflict("generated output: too many entries".into()));
        }
    }
    Ok(())
}

fn hash_file(path: &Path, size: u64) -> Result<String> {
    if size > MAX_FILE as u64 {
        return Err(Error::Conflict(
            "generated output: file exceeds 32 MiB".into(),
        ));
    }
    let mut file = fs::File::open(path).context("open generated file")?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 64 * 1024];
    let mut total = 0;
    loop {
        let count = file.read(&mut buffer).context("hash generated file")?;
        if count == 0 {
            break;
        }
        total += count;
        if total > MAX_FILE {
            return Err(Error::Conflict(
                "generated output: file grew beyond 32 MiB".into(),
            ));
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(unix)]
pub fn executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
pub fn executable(_: &fs::Metadata) -> bool {
    false
}

fn sync_tree_dirs(path: &Path) -> Result<()> {
    for entry in fs::read_dir(path).context("read staged directories")? {
        let entry = entry.context("read staged directory entry")?;
        if entry
            .file_type()
            .context("inspect staged directory")?
            .is_dir()
        {
            sync_tree_dirs(&entry.path())?;
        }
    }
    sync_dir(path)
}

pub fn sync_dir(path: &Path) -> Result<()> {
    fs::File::open(path)
        .context("open directory for flush")?
        .sync_all()
        .context("flush directory")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn fingerprints_include_paths_modes_and_empty_directories() {
        let temp = tempfile::tempdir().unwrap();
        let mut tree = Tree::new();
        tree.insert(
            "a/b.txt".into(),
            FileContent {
                bytes: Arc::from(&b"hello"[..]),
                executable: false,
            },
        );
        tree.write(&temp.path().join("tree")).unwrap();
        assert_eq!(
            Some(tree.fingerprint()),
            fingerprint_path(&temp.path().join("tree")).unwrap()
        );
        fs::create_dir(temp.path().join("tree/extra")).unwrap();
        assert_ne!(
            Some(tree.fingerprint()),
            fingerprint_path(&temp.path().join("tree")).unwrap()
        );
    }

    #[test]
    fn rejects_case_and_unicode_normalisation_collisions() {
        for (first, second) in [
            ("File.md", "file.md"),
            ("caf\u{e9}.md", "cafe\u{301}.md"),
            ("Folder/a.md", "folder/b.md"),
        ] {
            let mut tree = Tree::new();
            for path in [first, second] {
                tree.insert(
                    path.into(),
                    FileContent {
                        bytes: Arc::from(&b"x"[..]),
                        executable: false,
                    },
                );
            }
            assert!(tree.validate().is_err());
        }
    }
}
