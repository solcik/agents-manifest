use crate::{
    content::{FileContent, MAX_FILE, MAX_FILES, MAX_TREE, Tree},
    error::{Error, IoContext, Result},
    manifest::{self, Skill, SourceReference},
};
use clap::ValueEnum;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    io::{BufRead, Cursor, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
use wait_timeout::ChildExt;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum GitMode {
    #[default]
    Auto,
    System,
    Agent,
}

#[derive(Clone)]
pub struct GitOptions {
    pub cache: PathBuf,
    pub offline: bool,
    pub mode: GitMode,
    pub timeout: Duration,
}

#[derive(Default)]
struct RepositorySlot {
    path: Mutex<Option<PathBuf>>,
}

pub struct GitSource {
    options: GitOptions,
    repositories: Mutex<BTreeMap<String, Arc<RepositorySlot>>>,
}

pub trait Source: Sync {
    fn file(&self, reference: &SourceReference) -> Result<Vec<u8>>;
    fn skill(&self, skill: &Skill) -> Result<Tree>;
}

fn available(binary: &str) -> bool {
    env::var_os("PATH")
        .is_some_and(|paths| env::split_paths(&paths).any(|path| path.join(binary).is_file()))
}

pub fn default_cache() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            return Ok(path.join("agents-manifest"));
        }
    }
    env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".cache/agents-manifest"))
        .ok_or_else(|| Error::Invalid("cache: set HOME or provide --cache-dir".into()))
}

impl GitSource {
    pub fn new(options: GitOptions) -> Self {
        Self {
            options,
            repositories: Mutex::new(BTreeMap::new()),
        }
    }
    fn command(&self, directory: &Path, remote: Option<&str>) -> Result<Command> {
        let mut command = Command::new("git");
        if let Some(source) = remote {
            let agent = match self.options.mode {
                GitMode::Auto => available("git-agent"),
                GitMode::Agent => true,
                GitMode::System => false,
            };
            if agent {
                let parsed = url::Url::parse(source)
                    .map_err(|_| Error::Invalid("source: invalid Git URL".into()))?;
                let scope = match parsed.host_str() {
                    Some("github.com") => "github",
                    Some("git.vs-point.cz") => "vspoint",
                    _ => {
                        return Err(Error::Source(
                            "transport: agent wrapper has no scope for this host".into(),
                        ));
                    }
                };
                command.args(["agent", &format!("--scope={scope}")]);
            }
        }
        command
            .current_dir(directory)
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
                "-c",
                "protocol.ext.allow=never",
                "-c",
                "protocol.file.allow=never",
                "-c",
                "core.sshCommand=ssh -oBatchMode=yes",
                "-c",
                "credential.interactive=false",
            ])
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env("GIT_ASKPASS", "/bin/false")
            .env("SSH_ASKPASS", "/bin/false")
            .env("LC_ALL", "C")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        Ok(command)
    }

    fn run(&self, mut command: Command, input: Option<Vec<u8>>) -> Result<Vec<u8>> {
        if input.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command
            .spawn()
            .map_err(|_| Error::Source("Git: cannot start the transport".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Internal("Git stdout is unavailable".into()))?;
        let output_thread = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take(MAX_TREE as u64 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let input_thread = input.map(|bytes| {
            let stdin = child.stdin.take();
            std::thread::spawn(move || -> std::io::Result<()> {
                if let Some(mut stdin) = stdin {
                    stdin.write_all(&bytes)?;
                }
                Ok(())
            })
        });
        let status = child
            .wait_timeout(self.options.timeout)
            .context("wait for Git")?;
        let timed_out = status.is_none();
        let status = match status {
            Some(status) => status,
            None => {
                #[cfg(unix)]
                {
                    use nix::{
                        sys::signal::{Signal, killpg},
                        unistd::Pid,
                    };
                    let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
                }
                let _ = child.kill();
                child.wait().context("reap Git transport")?
            }
        };
        let output = output_thread
            .join()
            .map_err(|_| Error::Internal("Git reader failed".into()))?
            .map_err(|_| Error::Source("Git: cannot read object output".into()))?;
        if let Some(thread) = input_thread {
            thread
                .join()
                .map_err(|_| Error::Internal("Git writer failed".into()))?
                .map_err(|_| Error::Source("Git: cannot send object requests".into()))?;
        }
        if timed_out {
            return Err(Error::Source("Git: operation exceeded --timeout".into()));
        }
        if !status.success() {
            return Err(Error::Source(
                "Git: operation failed; check source access and revision".into(),
            ));
        }
        if output.len() > MAX_TREE {
            return Err(Error::Source("Git: object output exceeds 128 MiB".into()));
        }
        Ok(output)
    }

    fn read(&self, directory: &Path, arguments: &[&str]) -> Result<Vec<u8>> {
        let mut command = self.command(directory, None)?;
        command.args(arguments);
        self.run(command, None)
    }

    fn repository(&self, reference: &SourceReference) -> Result<PathBuf> {
        let slot = self
            .repositories
            .lock()
            .map_err(|_| Error::Internal("repository cache lock failed".into()))?
            .entry(reference.cache_key())
            .or_default()
            .clone();
        let mut path = slot
            .path
            .lock()
            .map_err(|_| Error::Internal("source cache lock failed".into()))?;
        if let Some(path) = &*path {
            return Ok(path.clone());
        }
        let repository = self.repository_disk(reference)?;
        *path = Some(repository.clone());
        Ok(repository)
    }

    fn repository_disk(&self, reference: &SourceReference) -> Result<PathBuf> {
        let source = reference.source();
        let revision = reference.revision();
        let key = reference.cache_key();
        let repository = self.options.cache.join(&key);
        if repository.is_dir() {
            self.verify(&repository, revision)?;
            return Ok(repository);
        }
        if self.options.offline {
            return Err(Error::Source(
                "cache: pinned source is unavailable offline".into(),
            ));
        }
        fs::create_dir_all(&self.options.cache).context("create source cache")?;
        let temporary = tempfile::Builder::new()
            .prefix("fetch-")
            .tempdir_in(&self.options.cache)
            .context("create temporary cache")?;
        let mut init = self.command(temporary.path(), None)?;
        init.args([
            "init",
            "--bare",
            if revision.len() == 64 {
                "--object-format=sha256"
            } else {
                "--object-format=sha1"
            },
        ]);
        self.run(init, None)?;
        let mut fetch = self.command(temporary.path(), Some(source))?;
        fetch.args(["fetch", "--no-tags", "--depth=1", "--", source, revision]);
        self.run(fetch, None)?;
        self.verify(temporary.path(), revision)?;
        // A competing resolver can publish the same immutable cache first.
        match fs::rename(temporary.path(), &repository) {
            Ok(()) => {}
            Err(_) if repository.is_dir() => {
                self.verify(&repository, revision)?;
            }
            Err(source) => {
                return Err(Error::Io {
                    operation: "publish source cache",
                    source,
                });
            }
        }
        Ok(repository)
    }

    fn verify(&self, repository: &Path, revision: &str) -> Result<()> {
        let metadata =
            fs::symlink_metadata(repository).context("inspect source cache directory")?;
        if !metadata.is_dir()
            || self.read(repository, &["rev-parse", "--is-bare-repository"])? != b"true\n"
        {
            return Err(Error::Source(
                "cache: require a regular bare repository directory".into(),
            ));
        }
        let fetched = self.read(repository, &["rev-parse", "--verify", "FETCH_HEAD"])?;
        if String::from_utf8_lossy(&fetched).trim() != revision {
            return Err(Error::Source(
                "cache: fetched commit does not match the declared revision".into(),
            ));
        }
        let kind = self.read(repository, &["cat-file", "-t", revision])?;
        if kind != b"commit\n" {
            return Err(Error::Source(
                "revision: source must identify a commit".into(),
            ));
        }
        self.read(repository, &["fsck", "--strict", "--no-reflogs"])?;
        Ok(())
    }
}

impl Source for GitSource {
    fn file(&self, reference: &SourceReference) -> Result<Vec<u8>> {
        let repository = self.repository(reference)?;
        let object = reference.object();
        let listing = self.read(
            &repository,
            &[
                "ls-tree",
                "-z",
                reference.revision(),
                "--",
                reference.path(),
            ],
        )?;
        let GitTreeEntry { mode, .. } =
            GitTreeEntry::parse(listing.strip_suffix(&[0]).unwrap_or(&listing))?;
        if mode != "100644" && mode != "100755" {
            return Err(Error::Source("bundle: require a regular file".into()));
        }
        let bytes = self.read(&repository, &["cat-file", "blob", &object])?;
        if bytes.len() > manifest::MAX_MANIFEST {
            return Err(Error::Invalid("bundle: maximum size is 1 MiB".into()));
        }
        Ok(bytes)
    }

    fn skill(&self, skill: &Skill) -> Result<Tree> {
        let repository = self.repository(&skill.reference)?;
        let root = skill.reference.object();
        let kind = self.read(&repository, &["cat-file", "-t", &root])?;
        if kind != b"tree\n" {
            return Err(Error::Source(
                "skill: selected path must identify a directory".into(),
            ));
        }
        let listing = self.read(&repository, &["ls-tree", "-r", "-t", "-z", &root])?;
        let mut files = Vec::new();
        let mut objects = BTreeSet::new();
        let mut count = 0;
        for raw in listing.split(|b| *b == 0).filter(|raw| !raw.is_empty()) {
            count += 1;
            if count > MAX_FILES {
                return Err(Error::Source("skill: maximum entry count is 10000".into()));
            }
            let GitTreeEntry {
                path,
                mode,
                kind,
                oid,
            } = GitTreeEntry::parse(raw)?;
            manifest::relative_path(&path)
                .map_err(|_| Error::Source("skill: unsafe source path".into()))?;
            match (mode.as_str(), kind.as_str()) {
                ("040000", "tree") => {}
                ("100644" | "100755", "blob") => {
                    objects.insert(oid.clone());
                    files.push((path, mode == "100755", oid));
                }
                _ => {
                    return Err(Error::Source(
                        "skill: symlinks, submodules, and special files are forbidden".into(),
                    ));
                }
            }
        }
        let requests = objects
            .iter()
            .map(|oid| format!("{oid}\n"))
            .collect::<String>()
            .into_bytes();
        let mut command = self.command(&repository, None)?;
        command.args(["cat-file", "--batch"]);
        let output = self.run(command, Some(requests))?;
        let mut cursor = Cursor::new(output);
        let mut blobs = BTreeMap::<String, Arc<[u8]>>::new();
        let mut total = 0;
        for expected in objects {
            let mut header = String::new();
            cursor
                .read_line(&mut header)
                .context("read Git object header")?;
            let parts: Vec<_> = header.split_whitespace().collect();
            if parts.len() != 3 || parts[0] != expected || parts[1] != "blob" {
                return Err(Error::Source("Git: invalid batch object header".into()));
            }
            let size: usize = parts[2]
                .parse()
                .map_err(|_| Error::Source("Git: invalid object size".into()))?;
            if size > MAX_FILE || total > MAX_TREE - size {
                return Err(Error::Source(
                    "skill: content exceeds the size limit".into(),
                ));
            }
            total += size;
            let mut bytes = vec![0; size];
            cursor.read_exact(&mut bytes).context("read Git object")?;
            let mut newline = [0];
            cursor
                .read_exact(&mut newline)
                .context("read Git object separator")?;
            if newline != *b"\n" {
                return Err(Error::Source("Git: invalid object separator".into()));
            }
            blobs.insert(expected, Arc::from(bytes));
        }
        let mut tree = Tree::new();
        let mut expanded = 0;
        for (path, executable, oid) in files {
            let bytes = blobs
                .get(&oid)
                .ok_or_else(|| Error::Internal("missing resolved blob".into()))?
                .clone();
            expanded += bytes.len();
            if expanded > MAX_TREE {
                return Err(Error::Source("skill: expanded tree exceeds 128 MiB".into()));
            }
            tree.insert(path, FileContent { bytes, executable });
        }
        tree.validate()?;
        let body = tree
            .get("SKILL.md")
            .ok_or_else(|| Error::Source("skill: selected directory lacks SKILL.md".into()))?;
        validate_frontmatter(&body.bytes, &skill.name)?;
        Ok(tree)
    }
}

struct GitTreeEntry {
    path: String,
    mode: String,
    kind: String,
    oid: String,
}

impl GitTreeEntry {
    fn parse(raw: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(raw)
            .map_err(|_| Error::Source("Git: require UTF-8 source paths".into()))?;
        let (header, path) = text
            .split_once('\t')
            .ok_or_else(|| Error::Source("Git: invalid tree entry".into()))?;
        let parts: Vec<_> = header.split_whitespace().collect();
        if parts.len() != 3 {
            return Err(Error::Source("Git: invalid tree entry header".into()));
        }
        Ok(Self {
            path: path.into(),
            mode: parts[0].into(),
            kind: parts[1].into(),
            oid: parts[2].into(),
        })
    }
}

pub fn validate_frontmatter(bytes: &[u8], expected: &str) -> Result<()> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::Source("SKILL.md: require UTF-8 text".into()))?;
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return Err(Error::Source("SKILL.md: require YAML frontmatter".into()));
    }
    let mut frontmatter = String::new();
    let mut closed = false;
    for line in lines {
        if line == "---" {
            closed = true;
            break;
        }
        frontmatter.push_str(line);
        frontmatter.push('\n');
        if frontmatter.len() > manifest::MAX_MANIFEST {
            return Err(Error::Source("SKILL.md: frontmatter exceeds 1 MiB".into()));
        }
    }
    #[derive(serde::Deserialize)]
    struct Metadata {
        name: String,
        description: String,
    }
    let metadata: Metadata = manifest::parse(frontmatter.as_bytes(), "SKILL.md frontmatter")
        .map_err(|_| Error::Source("SKILL.md: invalid YAML frontmatter".into()))?;
    if !closed || metadata.name != expected || metadata.description.trim().is_empty() {
        return Err(Error::Source(
            "SKILL.md: require the declared name and a nonempty description".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::test_support::FixtureRepository;

    fn cached_skill(mode: &str) -> (tempfile::TempDir, GitSource, Skill) {
        let fixture = FixtureRepository::new();
        let body = fixture.object(
            "blob",
            b"---\nname: sample\ndescription: Test skill\n---\nHello\n",
        );
        let binary = fixture.object("blob", &[0, b'\n', 255, 12]);
        let directory = fixture.tree(&[("100644", "SKILL.md", &body), (mode, "tool", &binary)]);
        let root = fixture.tree(&[("40000", "sample", &directory)]);
        let revision = fixture.commit(&root);
        let skill = Skill {
            name: "sample".into(),
            reference: SourceReference::new(
                "https://github.com/example/skills",
                revision,
                "sample",
            )
            .unwrap(),
            targets: None,
            activation: None,
        };
        let temporary = tempfile::tempdir().unwrap();
        let cached = temporary.path().join(skill.reference.cache_key());
        fs::rename(fixture.path(), cached).unwrap();
        let source = GitSource::new(GitOptions {
            cache: temporary.path().to_owned(),
            offline: true,
            mode: GitMode::System,
            timeout: Duration::from_secs(5),
        });
        (temporary, source, skill)
    }

    #[test]
    fn reads_cached_git_blobs_with_binary_data_and_executable_modes() {
        let (_temporary, source, skill) = cached_skill("100755");
        let tree = source.skill(&skill).unwrap();
        let tool = tree.get("tool").unwrap();
        assert_eq!(&*tool.bytes, &[0, b'\n', 255, 12]);
        assert!(tool.executable);
    }

    #[test]
    fn rejects_source_symlinks_and_submodules() {
        for mode in ["120000", "160000"] {
            let (_temporary, source, skill) = cached_skill(mode);
            assert!(matches!(source.skill(&skill), Err(Error::Source(_))));
        }
    }

    #[test]
    fn offline_cache_miss_has_source_exit_status() {
        let (_temporary, mut source, skill) = cached_skill("100644");
        source.options.cache = source.options.cache.join("missing");
        assert_eq!(source.skill(&skill).unwrap_err().code(), 3);
        assert!(!source.options.cache.exists());
    }

    #[test]
    fn rejects_corrupt_revision_metadata() {
        let (_temporary, source, skill) = cached_skill("100644");
        fs::write(
            source
                .options
                .cache
                .join(skill.reference.cache_key())
                .join("FETCH_HEAD"),
            format!("{}\n", "0".repeat(40)),
        )
        .unwrap();
        assert!(matches!(source.skill(&skill), Err(Error::Source(_))));
    }

    #[test]
    fn frontmatter_names_and_descriptions_are_required() {
        for bytes in [
            b"No frontmatter".as_slice(),
            b"---\nname: other\ndescription: Test\n---",
            b"---\nname: sample\ndescription: ''\n---",
        ] {
            assert!(validate_frontmatter(bytes, "sample").is_err());
        }
        assert!(
            validate_frontmatter(
                b"---\r\nname: sample\r\ndescription: Test\r\n---\r\nBody",
                "sample"
            )
            .is_ok()
        );
    }

    #[cfg(unix)]
    #[test]
    fn timeout_terminates_the_process_group_and_pipe_readers() {
        use std::os::unix::process::CommandExt;
        let (_temporary, mut source, _skill) = cached_skill("100644");
        source.options.timeout = Duration::from_millis(100);
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 10 & wait"])
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let started = std::time::Instant::now();
        assert_eq!(source.run(command, None).unwrap_err().code(), 3);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
