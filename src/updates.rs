use crate::{
    error::{Error, IoContext, Result},
    manifest::{MAX_MANIFEST, Manifest, Skill, SourceReference, ValidatedManifest},
    selectors::{self, Loaded, Selector},
    source::Source,
};
use serde::Serialize;
use std::{collections::BTreeMap, fs, io::Write, path::Path};

pub trait UpdateSource: Source {
    fn head(&self, source: &str) -> Result<String>;
    fn selected(&self, source: &str, selector: &Selector) -> Result<String>;
}

impl UpdateSource for crate::source::GitSource {
    fn head(&self, source: &str) -> Result<String> {
        self.head(source)
    }
    fn selected(&self, source: &str, selector: &Selector) -> Result<String> {
        selectors::resolve(self, source, selector)
    }
}

#[derive(Debug, Serialize)]
pub struct Update {
    pub name: String,
    pub source: String,
    pub pinned: Option<String>,
    pub target: String,
    pub status: &'static str,
    pub compare: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Preview {
    pub version: u8,
    pub updates: Vec<Update>,
}

pub fn preview(loaded: &Loaded, source: &impl UpdateSource) -> Result<Preview> {
    let mut heads = BTreeMap::new();
    let mut updates = Vec::new();
    for skill in &loaded.manifest.manifest.skills {
        let reference = &skill.reference;
        let selector = loaded.selectors.get(&skill.name);
        let key = (reference.source().to_owned(), selector.cloned());
        let target = heads
            .entry(key)
            .or_insert_with(|| match selector {
                Some(selector) => source.selected(reference.source(), selector),
                None => source.head(reference.source()),
            })
            .as_ref()
            .map_err(|error| Error::Source(error.to_string()))?;
        let pinned = if selector.is_some() {
            loaded
                .lock
                .skills
                .get(&skill.name)
                .map(|pin| pin.revision.clone())
        } else {
            Some(reference.revision().to_owned())
        };
        let status = if pinned.is_none() {
            match source.skill(&target_skill(skill, target)?) {
                Ok(_) => "unlocked",
                Err(_) => "unavailable",
            }
        } else if target == reference.revision() {
            match source.skill(skill) {
                Ok(_) => "current",
                Err(_) => "unavailable",
            }
        } else {
            let next = target_skill(skill, target)?;
            match source.skill(&next) {
                Err(_) => "unavailable",
                Ok(target_tree) => match source.skill(skill) {
                    Ok(pinned_tree) if pinned_tree.fingerprint() == target_tree.fingerprint() => {
                        "unchanged"
                    }
                    Ok(_) => "changed",
                    Err(_) => "unavailable",
                },
            }
        };
        updates.push(Update {
            name: skill.name.clone(),
            source: reference.source().to_owned(),
            pinned: pinned.clone(),
            target: target.clone(),
            status,
            compare: pinned
                .as_deref()
                .and_then(|from| github_compare(reference.source(), from, target)),
        });
    }
    Ok(Preview {
        version: 1,
        updates,
    })
}

pub fn apply(
    path: &Path,
    manifest: &ValidatedManifest,
    source: &impl Source,
    name: &str,
    from: &str,
    to: &str,
) -> Result<()> {
    let skill = manifest
        .manifest
        .skills
        .iter()
        .find(|skill| skill.name == name)
        .ok_or_else(|| Error::Invalid("updates: selected skill is not declared".into()))?;
    if skill.reference.revision() != from {
        return Err(Error::Conflict(
            "updates: pinned revision changed since review".into(),
        ));
    }
    if from == to {
        return Err(Error::Invalid(
            "updates: target matches the pinned revision".into(),
        ));
    }
    let target = target_skill(skill, to)?;
    source.skill(&target)?;
    let original = fs::read(path).context("read manifest for pin update")?;
    if original.len() > MAX_MANIFEST {
        return Err(Error::Invalid("manifest: maximum size is 1 MiB".into()));
    }
    let changed = replace_revision(&original, name, from, to)?;
    let validated = crate::manifest::parse::<Manifest>(&changed, "manifest")?.validate()?;
    for (before, after) in manifest
        .manifest
        .skills
        .iter()
        .zip(&validated.manifest.skills)
    {
        if before.name != after.name
            || before.reference.source() != after.reference.source()
            || before.reference.path() != after.reference.path()
            || (before.name != name && before.reference.revision() != after.reference.revision())
        {
            return Err(Error::Conflict(
                "updates: manifest structure changed".into(),
            ));
        }
    }
    if validated.manifest.skills.len() != manifest.manifest.skills.len()
        || validated.manifest.bundles.len() != manifest.manifest.bundles.len()
    {
        return Err(Error::Conflict(
            "updates: manifest structure changed".into(),
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("manifest: no parent directory".into()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).context("stage manifest update")?;
    let permissions = fs::metadata(path)
        .context("inspect manifest permissions")?
        .permissions();
    temporary
        .as_file()
        .set_permissions(permissions)
        .context("set manifest permissions")?;
    temporary
        .write_all(&changed)
        .context("stage manifest update")?;
    temporary
        .as_file()
        .sync_all()
        .context("sync manifest update")?;
    if fs::read(path).context("recheck manifest before update")? != original {
        return Err(Error::Conflict(
            "updates: manifest changed during update".into(),
        ));
    }
    temporary.persist(path).map_err(|error| Error::Io {
        operation: "publish manifest update",
        source: error.error,
    })?;
    Ok(())
}

pub fn apply_selected<S: UpdateSource>(
    path: &Path,
    loaded: &Loaded,
    source: &S,
    name: &str,
    from: Option<&str>,
    to: &str,
) -> Result<()> {
    let Some(selector) = loaded.selectors.get(name) else {
        let from = from.ok_or_else(|| {
            Error::Invalid("updates: --from is required for revision pins".into())
        })?;
        return apply(path, &loaded.manifest, source, name, from, to);
    };
    let skill = loaded
        .manifest
        .manifest
        .skills
        .iter()
        .find(|skill| skill.name == name)
        .ok_or_else(|| Error::Invalid("updates: selected skill is not declared".into()))?;
    let previous = loaded.lock.skills.get(name);
    if previous.map(|pin| pin.revision.as_str()) != from {
        return Err(Error::Conflict(
            "updates: locked revision changed since review".into(),
        ));
    }
    if previous.is_some_and(|pin| pin.revision == to) {
        return Err(Error::Invalid(
            "updates: target matches the locked revision".into(),
        ));
    }
    let target = source.selected(skill.reference.source(), selector)?;
    if target != to {
        return Err(Error::Conflict(
            "updates: selector target changed since review".into(),
        ));
    }
    let tree = source.skill(&target_skill(skill, to)?)?;
    let mut lock = loaded.lock.clone();
    lock.skills
        .retain(|name, _| loaded.selectors.contains_key(name));
    lock.skills.insert(
        name.to_owned(),
        selectors::LockedPin {
            source: skill.reference.source().to_owned(),
            path: skill.reference.path().to_owned(),
            selector: selector.clone(),
            revision: to.to_owned(),
            hash: tree.fingerprint().hash,
        },
    );
    if fs::read(path).context("recheck manifest before lock update")? != loaded.raw
        || selectors::current_lock(path)? != loaded.lock
    {
        return Err(Error::Conflict(
            "updates: manifest or lockfile changed during update".into(),
        ));
    }
    selectors::write_lock(path, &lock)
}

fn target_skill(skill: &Skill, revision: &str) -> Result<Skill> {
    let mut result = skill.clone();
    result.reference =
        SourceReference::new(skill.reference.source(), revision, skill.reference.path())?;
    Ok(result)
}

fn github_compare(source: &str, from: &str, to: &str) -> Option<String> {
    let url = url::Url::parse(source).ok()?;
    if url.host_str()? != "github.com" {
        return None;
    }
    let path = url.path().trim_matches('/').trim_end_matches(".git");
    let mut parts = path.split('/');
    let (Some(owner), Some(repo), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!(
        "https://github.com/{owner}/{repo}/compare/{from}...{to}"
    ))
}

// Edit only a plain revision scalar in the selected block-style skill entry.
// Refuse unfamiliar YAML layouts instead of rewriting comments or formatting.
fn replace_revision(bytes: &[u8], name: &str, from: &str, to: &str) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::Invalid("manifest: require UTF-8 for pin updates".into()))?;
    let mut in_skills = false;
    let mut entry = String::new();
    let mut selected = false;
    let mut matches = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if !line.starts_with([' ', '\t']) && trimmed.starts_with("skills:") {
            in_skills = true;
        } else if !line.starts_with([' ', '\t'])
            && !trimmed.starts_with('#')
            && !trimmed.trim().is_empty()
        {
            in_skills = false;
        }
        if in_skills && trimmed.starts_with("- ") {
            entry.clear();
            selected = false;
        }
        if in_skills {
            let field = trimmed.strip_prefix("- ").unwrap_or(trimmed);
            if let Some(value) = field.strip_prefix("name:") {
                let value = scalar(value);
                entry = value.to_owned();
                selected = entry == name;
            }
            if selected && let Some(value) = field.strip_prefix("revision:") {
                let value_start = line.len() - field.len() + "revision:".len();
                let rest = &line[value_start..];
                let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
                let scalar_start = value_start + spaces;
                let raw = &line[scalar_start..];
                let quote = raw.chars().next().filter(|c| *c == '\'' || *c == '"');
                let start = scalar_start + usize::from(quote.is_some());
                if !scalar(value).eq_ignore_ascii_case(from)
                    || !raw[start - scalar_start..].starts_with(from)
                {
                    return Err(Error::Conflict(
                        "updates: revision scalar does not match the reviewed pin".into(),
                    ));
                }
                matches.push((offset + start, offset + start + from.len()));
            }
        }
        offset += line.len();
    }
    if matches.len() != 1 {
        return Err(Error::Invalid(
            "updates: require one block-style revision for the selected skill".into(),
        ));
    }
    let (start, end) = matches[0];
    let mut result = bytes.to_vec();
    result.splice(start..end, to.bytes());
    Ok(result)
}

fn scalar(value: &str) -> &str {
    let value = value.trim();
    let value = value.split_once(" #").map_or(value, |(value, _)| value);
    value.trim_matches(['\'', '"'])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::content::{FileContent, Tree};
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };

    struct FakeSource {
        heads: BTreeMap<String, String>,
        calls: Mutex<Vec<String>>,
    }

    impl Source for FakeSource {
        fn file(&self, _: &SourceReference) -> Result<Vec<u8>> {
            unreachable!()
        }
        fn skill(&self, skill: &Skill) -> Result<Tree> {
            if skill.reference.path() == "removed" {
                return Err(Error::Source("missing path".into()));
            }
            let mut tree = Tree::new();
            let content = format!("{}:{}", skill.name, skill.reference.revision());
            tree.insert(
                "SKILL.md".into(),
                FileContent {
                    bytes: Arc::from(content.into_bytes()),
                    executable: false,
                },
            );
            Ok(tree)
        }
    }

    impl UpdateSource for FakeSource {
        fn head(&self, source: &str) -> Result<String> {
            self.calls.lock().unwrap().push(source.into());
            Ok(self.heads[source].clone())
        }
        fn selected(&self, source: &str, _: &Selector) -> Result<String> {
            self.head(source)
        }
    }

    fn manifest() -> ValidatedManifest {
        let text = format!(
            "version: 1\ntargets: [codex]\nskills:\n  - name: one\n    source: https://github.com/example/repo.git\n    revision: '{}'\n    path: one\n  - name: two\n    source: https://github.com/example/repo.git\n    revision: '{}'\n    path: removed\n  - name: three\n    source: https://git.example.org/team/repo.git\n    revision: '{}'\n    path: three\n",
            "a".repeat(40),
            "a".repeat(40),
            "a".repeat(40)
        );
        crate::manifest::parse::<Manifest>(text.as_bytes(), "manifest")
            .unwrap()
            .validate()
            .unwrap()
    }

    #[test]
    fn preview_deduplicates_sources_and_reports_missing_paths() {
        let mut heads = BTreeMap::new();
        heads.insert("https://github.com/example/repo.git".into(), "b".repeat(40));
        heads.insert(
            "https://git.example.org/team/repo.git".into(),
            "c".repeat(40),
        );
        let source = FakeSource {
            heads,
            calls: Mutex::new(Vec::new()),
        };
        let loaded = Loaded {
            raw: Vec::new(),
            manifest: manifest(),
            selectors: BTreeMap::new(),
            lock: selectors::Lockfile {
                version: 1,
                ..selectors::Lockfile::default()
            },
        };
        let result = preview(&loaded, &source).unwrap();
        assert_eq!(source.calls.lock().unwrap().len(), 2);
        assert_eq!(
            result
                .updates
                .iter()
                .map(|item| item.status)
                .collect::<Vec<_>>(),
            ["changed", "unavailable", "changed"]
        );
        let compare = format!(
            "https://github.com/example/repo/compare/{}...{}",
            "a".repeat(40),
            "b".repeat(40)
        );
        assert_eq!(result.updates[0].compare.as_deref(), Some(compare.as_str()));
        assert_eq!(result.updates[2].compare, None);
    }

    #[test]
    fn replacement_preserves_comments_and_other_revisions() {
        let from = "a".repeat(40);
        let to = "b".repeat(40);
        let original = format!(
            "# header\nversion: 1\ntargets: [codex]\nskills:\n  - name: one # selected\n    source: https://github.com/example/repo.git\n    revision: '{from}' # keep\n    path: one\n  - name: two\n    source: https://github.com/example/repo.git\n    revision: \"{from}\"\n    path: two\n"
        );
        let updated =
            String::from_utf8(replace_revision(original.as_bytes(), "one", &from, &to).unwrap())
                .unwrap();
        assert_eq!(
            updated,
            original.replacen(&format!("'{from}' # keep"), &format!("'{to}' # keep"), 1)
        );
        assert!(replace_revision(original.as_bytes(), "missing", &from, &to).is_err());
    }

    #[test]
    fn apply_requires_reviewed_pin_and_keeps_manifest_on_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("skills.yaml");
        let original = format!(
            "version: 1\ntargets: [codex]\nskills:\n  - name: one\n    source: https://github.com/example/repo.git\n    revision: '{}'\n    path: one\n",
            "a".repeat(40)
        );
        fs::write(&path, &original).unwrap();
        let input = crate::manifest::parse::<Manifest>(original.as_bytes(), "manifest")
            .unwrap()
            .validate()
            .unwrap();
        let source = FakeSource {
            heads: BTreeMap::new(),
            calls: Mutex::new(Vec::new()),
        };
        assert!(
            apply(
                &path,
                &input,
                &source,
                "one",
                &"c".repeat(40),
                &"b".repeat(40)
            )
            .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        apply(
            &path,
            &input,
            &source,
            "one",
            &"a".repeat(40),
            &"b".repeat(40),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            original.replace(&"a".repeat(40), &"b".repeat(40))
        );
    }

    #[test]
    fn selector_apply_creates_a_lock_without_rewriting_yaml() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("skills.yaml");
        let manifest = "# keep this comment\nversion: 1\ntargets: [codex]\nskills:\n  - name: sample\n    source: https://github.com/example/repo.git\n    branch: main\n    path: sample\n";
        fs::write(&path, manifest).unwrap();
        let loaded = selectors::load(&path, selectors::LoadMode::Preview).unwrap();
        let source = FakeSource {
            heads: BTreeMap::from([("https://github.com/example/repo.git".into(), "a".repeat(40))]),
            calls: Mutex::new(Vec::new()),
        };
        assert!(apply_selected(&path, &loaded, &source, "sample", None, &"b".repeat(40)).is_err());
        assert!(!directory.path().join(selectors::LOCKFILE).exists());
        apply_selected(&path, &loaded, &source, "sample", None, &"a".repeat(40)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), manifest);
        let locked = selectors::load(&path, selectors::LoadMode::Locked).unwrap();
        assert_eq!(locked.lock.skills["sample"].revision, "a".repeat(40));
        assert_eq!(locked.lock.skills["sample"].hash.len(), 64);
        assert!(apply_selected(&path, &locked, &source, "sample", None, &"a".repeat(40)).is_err());
    }
}
