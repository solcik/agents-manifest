use crate::{
    error::{Error, IoContext, Result},
    manifest::{self, MAX_MANIFEST, Manifest, Skill, SourceReference, ValidatedManifest},
    source::{GitSource, Source},
};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const LOCKFILE: &str = "skills-lock.json";
const PLACEHOLDER: &str = "0000000000000000000000000000000000000000";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum SelectorKind {
    Branch,
    Tag,
    Version,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    pub kind: SelectorKind,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedPin {
    pub source: String,
    pub path: String,
    pub selector: Selector,
    pub revision: String,
    pub hash: String,
}

#[derive(Default, Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Lockfile {
    pub version: u8,
    pub skills: BTreeMap<String, LockedPin>,
}

pub struct Loaded {
    pub raw: Vec<u8>,
    pub manifest: ValidatedManifest,
    pub selectors: BTreeMap<String, Selector>,
    pub lock: Lockfile,
}

#[derive(Clone, Copy)]
pub enum LoadMode {
    Validate,
    Preview,
    Locked,
}

pub fn load(path: &Path, mode: LoadMode) -> Result<Loaded> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| Error::Invalid("manifest: cannot read the requested file".into()))?;
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST as u64 {
        return Err(Error::Invalid(
            "manifest: require a regular file under 1 MiB".into(),
        ));
    }
    let bytes = crate::content::read_file(path, MAX_MANIFEST)
        .map_err(|_| Error::Invalid("manifest: cannot read the requested file".into()))?;
    let mut value: serde_yaml::Value = manifest::parse(&bytes, "manifest")?;
    let lock = if matches!(mode, LoadMode::Validate) {
        Lockfile {
            version: 1,
            ..Lockfile::default()
        }
    } else {
        read_lock(path)?
    };
    let mut selectors = BTreeMap::new();
    let entries = value
        .as_mapping_mut()
        .and_then(|map| map.get_mut("skills"))
        .and_then(serde_yaml::Value::as_sequence_mut)
        .ok_or_else(|| Error::Invalid("skills: require a list".into()))?;
    for (index, entry) in entries.iter_mut().enumerate() {
        let map = entry
            .as_mapping_mut()
            .ok_or_else(|| Error::Invalid(format!("skills[{index}]: require a mapping")))?;
        let mut selected = None;
        for (field, kind) in [
            ("branch", SelectorKind::Branch),
            ("tag", SelectorKind::Tag),
            ("version", SelectorKind::Version),
        ] {
            if let Some(value) = map.remove(field) {
                if selected.is_some() || map.contains_key("revision") {
                    return Err(Error::Invalid(format!(
                        "skills[{index}]: select one revision, branch, tag, or version"
                    )));
                }
                let text = value.as_str().ok_or_else(|| {
                    Error::Invalid(format!("skills[{index}].{field}: require text"))
                })?;
                validate_selector(&kind, text)?;
                selected = Some(Selector {
                    kind,
                    value: text.to_owned(),
                });
            }
        }
        if let Some(selector) = selected {
            let name = map
                .get("name")
                .and_then(serde_yaml::Value::as_str)
                .ok_or_else(|| Error::Invalid(format!("skills[{index}].name: require text")))?;
            if selectors.insert(name.to_owned(), selector).is_some() {
                return Err(Error::Invalid("skills: duplicate skill name".into()));
            }
            let revision = if matches!(mode, LoadMode::Validate) {
                PLACEHOLDER
            } else {
                lock.skills
                    .get(name)
                    .map_or(PLACEHOLDER, |pin| pin.revision.as_str())
            };
            map.insert("revision".into(), revision.into());
        }
    }
    let manifest: Manifest = serde_yaml::from_value(value)
        .map_err(|_| Error::Invalid("manifest: invalid skill structure".into()))?;
    let manifest = manifest.validate()?;
    if !matches!(mode, LoadMode::Validate) {
        validate_lock(
            &manifest,
            &selectors,
            &lock,
            matches!(mode, LoadMode::Locked),
        )?;
    }
    Ok(Loaded {
        raw: bytes,
        manifest,
        selectors,
        lock,
    })
}

pub fn current_lock(path: &Path) -> Result<Lockfile> {
    read_lock(path)
}

fn validate_selector(kind: &SelectorKind, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 200
        || value.starts_with('-')
        || value.contains("..")
        || value.contains("@{")
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._/-+^~<>=*, ".contains(&c))
    {
        return Err(Error::Invalid(
            "selector: invalid branch, tag, or version".into(),
        ));
    }
    if matches!(kind, SelectorKind::Version) {
        VersionReq::parse(value)
            .map_err(|_| Error::Invalid("version: require a SemVer range".into()))?;
    } else if value.bytes().any(|c| b" +^~<>=*,".contains(&c)) || value.ends_with('/') {
        return Err(Error::Invalid("selector: invalid Git ref name".into()));
    }
    Ok(())
}

fn lock_path(manifest: &Path) -> Result<PathBuf> {
    Ok(manifest
        .parent()
        .ok_or_else(|| Error::Invalid("manifest: no parent directory".into()))?
        .join(LOCKFILE))
}

fn read_lock(path: &Path) -> Result<Lockfile> {
    let path = lock_path(path)?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.is_file() || metadata.len() > MAX_MANIFEST as u64 => {
            return Err(Error::Invalid(
                "lockfile: require a regular file under 1 MiB".into(),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Lockfile {
                version: 1,
                ..Lockfile::default()
            });
        }
        Err(error) => {
            return Err(Error::Io {
                operation: "inspect skill lockfile",
                source: error,
            });
        }
        Ok(_) => {}
    }
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Lockfile {
                version: 1,
                ..Lockfile::default()
            });
        }
        Err(error) => {
            return Err(Error::Io {
                operation: "read skill lockfile",
                source: error,
            });
        }
    };
    if bytes.len() > MAX_MANIFEST {
        return Err(Error::Invalid("lockfile: maximum size is 1 MiB".into()));
    }
    let lock: Lockfile = serde_json::from_slice(&bytes)
        .map_err(|_| Error::Invalid("lockfile: invalid structure".into()))?;
    if lock.version != 1 {
        return Err(Error::Invalid("lockfile: unsupported version".into()));
    }
    Ok(lock)
}

fn validate_lock(
    manifest: &ValidatedManifest,
    selectors: &BTreeMap<String, Selector>,
    lock: &Lockfile,
    complete: bool,
) -> Result<()> {
    for skill in &manifest.manifest.skills {
        let Some(selector) = selectors.get(&skill.name) else {
            continue;
        };
        let Some(pin) = lock.skills.get(&skill.name) else {
            if complete {
                return Err(Error::Invalid(format!(
                    "lockfile: {} has no locked pin",
                    skill.name
                )));
            }
            continue;
        };
        if &pin.selector != selector
            || pin.source != skill.reference.source()
            || pin.path != skill.reference.path()
            || SourceReference::new(&pin.source, &pin.revision, &pin.path).is_err()
            || pin.hash.len() != 64
            || !pin.hash.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(Error::Conflict(format!(
                "lockfile: {} does not match the manifest",
                skill.name
            )));
        }
    }
    Ok(())
}

pub struct LockedSource<'a> {
    pub source: &'a GitSource,
    pub lock: &'a Lockfile,
    pub selectors: &'a BTreeMap<String, Selector>,
}

impl Source for LockedSource<'_> {
    fn file(&self, reference: &SourceReference) -> Result<Vec<u8>> {
        self.source.file(reference)
    }
    fn skill(&self, skill: &Skill) -> Result<crate::content::Tree> {
        let tree = self.source.skill(skill)?;
        if let Some(pin) = self.lock.skills.get(&skill.name)
            && self.selectors.contains_key(&skill.name)
            && pin.source == skill.reference.source()
            && pin.path == skill.reference.path()
            && pin.revision == skill.reference.revision()
            && pin.hash != tree.fingerprint().hash
        {
            return Err(Error::Source(format!(
                "lockfile: {} content hash differs",
                skill.name
            )));
        }
        Ok(tree)
    }
}

pub fn resolve(source: &GitSource, url: &str, selector: &Selector) -> Result<String> {
    let refs = match selector.kind {
        SelectorKind::Branch => {
            source.list_refs(url, &[&format!("refs/heads/{}", selector.value)])?
        }
        SelectorKind::Tag => source.list_refs(
            url,
            &[
                &format!("refs/tags/{}", selector.value),
                &format!("refs/tags/{}^{{}}", selector.value),
            ],
        )?,
        SelectorKind::Version => source.list_refs(url, &["refs/tags/*"])?,
    };
    select_ref(&refs, selector)
}

fn select_ref(refs: &BTreeMap<String, String>, selector: &Selector) -> Result<String> {
    match selector.kind {
        SelectorKind::Branch => refs.get(&format!("refs/heads/{}", selector.value)).cloned(),
        SelectorKind::Tag => refs
            .get(&format!("refs/tags/{}^{{}}", selector.value))
            .or_else(|| refs.get(&format!("refs/tags/{}", selector.value)))
            .cloned(),
        SelectorKind::Version => {
            let requirement = VersionReq::parse(&selector.value)
                .map_err(|_| Error::Invalid("version: require a SemVer range".into()))?;
            refs.iter()
                .filter_map(|(name, revision)| {
                    let raw = name.strip_prefix("refs/tags/")?;
                    if raw.ends_with("^{}") {
                        return None;
                    }
                    let version = Version::parse(raw.strip_prefix('v').unwrap_or(raw)).ok()?;
                    let peeled = refs.get(&format!("{name}^{{}}")).unwrap_or(revision);
                    requirement.matches(&version).then_some((version, peeled))
                })
                .max_by(|a, b| a.0.cmp(&b.0))
                .map(|(_, revision)| revision.clone())
        }
    }
    .ok_or_else(|| Error::Source("updates: selector has no matching upstream commit".into()))
}

pub fn write_lock(path: &Path, lock: &Lockfile) -> Result<()> {
    let destination = lock_path(path)?;
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Invalid("lockfile: no parent directory".into()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).context("stage skill lockfile")?;
    serde_json::to_writer_pretty(&mut temporary, lock)
        .map_err(|_| Error::Internal("cannot encode skill lockfile".into()))?;
    temporary.write_all(b"\n").context("stage skill lockfile")?;
    temporary
        .as_file()
        .sync_all()
        .context("sync skill lockfile")?;
    temporary.persist(&destination).map_err(|error| Error::Io {
        operation: "publish skill lockfile",
        source: error.error,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn selectors_choose_branches_peeled_tags_and_latest_semver() {
        let refs = BTreeMap::from([
            ("refs/heads/main".into(), "a".repeat(40)),
            ("refs/tags/v1.2.0".into(), "b".repeat(40)),
            ("refs/tags/v1.2.0^{}".into(), "c".repeat(40)),
            ("refs/tags/v1.3.0".into(), "d".repeat(40)),
            ("refs/tags/v2.0.0".into(), "e".repeat(40)),
        ]);
        let selected = |kind, value: &str| {
            select_ref(
                &refs,
                &Selector {
                    kind,
                    value: value.into(),
                },
            )
            .unwrap()
        };
        assert_eq!(selected(SelectorKind::Branch, "main"), "a".repeat(40));
        assert_eq!(selected(SelectorKind::Tag, "v1.2.0"), "c".repeat(40));
        assert_eq!(selected(SelectorKind::Version, "^1.2"), "d".repeat(40));
        assert!(
            select_ref(
                &refs,
                &Selector {
                    kind: SelectorKind::Version,
                    value: "^3".into()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn a_selector_requires_a_matching_lock_for_sync() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("skills.yaml");
        let body = "version: 1\ntargets: [codex]\nskills:\n  - name: sample\n    source: https://github.com/example/repo.git\n    branch: main\n    path: sample\n";
        fs::write(&path, body).unwrap();
        let preview = load(&path, LoadMode::Preview).unwrap();
        assert_eq!(preview.selectors["sample"].value, "main");
        assert!(load(&path, LoadMode::Locked).is_err());
        let mut lock = Lockfile {
            version: 1,
            ..Lockfile::default()
        };
        lock.skills.insert(
            "sample".into(),
            LockedPin {
                source: "https://github.com/example/repo.git".into(),
                path: "sample".into(),
                selector: preview.selectors["sample"].clone(),
                revision: "a".repeat(40),
                hash: "b".repeat(64),
            },
        );
        write_lock(&path, &lock).unwrap();
        assert_eq!(
            load(&path, LoadMode::Locked)
                .unwrap()
                .manifest
                .manifest
                .skills[0]
                .reference
                .revision(),
            "a".repeat(40)
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), body);
        fs::write(&path, body.replace("branch: main", "branch: develop")).unwrap();
        assert!(matches!(
            load(&path, LoadMode::Locked),
            Err(Error::Conflict(_))
        ));
    }

    #[test]
    fn invalid_selector_combinations_fail_validation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("skills.yaml");
        let base = "version: 1\ntargets: [codex]\nskills:\n  - name: sample\n    source: https://github.com/example/repo.git\n    path: sample\n";
        for fields in [
            "    branch: main\n    tag: v1\n",
            "    version: nonsense\n",
            "    branch: ../main\n",
        ] {
            fs::write(&path, format!("{base}{fields}")).unwrap();
            assert!(load(&path, LoadMode::Validate).is_err());
        }
    }
}
