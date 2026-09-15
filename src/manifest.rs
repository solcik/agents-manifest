use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

pub const MAX_MANIFEST: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Codex,
    Claude,
    Pi,
    Opencode,
    Hermes,
    All,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub targets: Vec<Target>,
    pub skills: Vec<Skill>,
    #[serde(default)]
    pub bundles: Vec<BundleRef>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(try_from = "RawSkill", into = "RawSkill")]
pub struct Skill {
    pub name: String,
    pub reference: SourceReference,
    pub targets: Option<Vec<Target>>,
    pub activation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(try_from = "RawBundleRef", into = "RawBundleRef")]
pub struct BundleRef {
    pub reference: SourceReference,
    pub targets: Option<Vec<Target>>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "RawSourceReference", into = "RawSourceReference")]
pub struct SourceReference {
    source: String,
    revision: String,
    path: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawSourceReference {
    source: String,
    revision: String,
    path: String,
}

impl TryFrom<RawSourceReference> for SourceReference {
    type Error = String;
    fn try_from(raw: RawSourceReference) -> std::result::Result<Self, Self::Error> {
        Self::new(raw.source, raw.revision, raw.path).map_err(|error| error.to_string())
    }
}

impl From<SourceReference> for RawSourceReference {
    fn from(reference: SourceReference) -> Self {
        Self {
            source: reference.source,
            revision: reference.revision,
            path: reference.path,
        }
    }
}

// Wire structs keep strict unknown-field checks without Serde flatten ambiguity.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawSkill {
    name: String,
    source: String,
    revision: String,
    path: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    targets: Option<Vec<Target>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    activation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RawBundleRef {
    source: String,
    revision: String,
    path: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    targets: Option<Vec<Target>>,
}

fn present<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl TryFrom<RawSkill> for Skill {
    type Error = String;
    fn try_from(raw: RawSkill) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            name: raw.name,
            reference: SourceReference::new(raw.source, raw.revision, raw.path)
                .map_err(|error| error.to_string())?,
            targets: raw.targets,
            activation: raw.activation,
        })
    }
}

impl From<Skill> for RawSkill {
    fn from(skill: Skill) -> Self {
        Self {
            name: skill.name,
            source: skill.reference.source,
            revision: skill.reference.revision,
            path: skill.reference.path,
            targets: skill.targets,
            activation: skill.activation,
        }
    }
}

impl TryFrom<RawBundleRef> for BundleRef {
    type Error = String;
    fn try_from(raw: RawBundleRef) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            reference: SourceReference::new(raw.source, raw.revision, raw.path)
                .map_err(|error| error.to_string())?,
            targets: raw.targets,
        })
    }
}

impl From<BundleRef> for RawBundleRef {
    fn from(bundle: BundleRef) -> Self {
        Self {
            source: bundle.reference.source,
            revision: bundle.reference.revision,
            path: bundle.reference.path,
            targets: bundle.targets,
        }
    }
}

impl SourceReference {
    pub fn new(
        source: impl Into<String>,
        revision: impl Into<String>,
        path: impl Into<String>,
    ) -> Result<Self> {
        let source = source.into();
        let revision = revision.into().to_ascii_lowercase();
        let path = path.into();
        reference(&source, &revision, &path)?;
        Ok(Self {
            source,
            revision,
            path,
        })
    }
    pub fn source(&self) -> &str {
        &self.source
    }
    pub fn revision(&self) -> &str {
        &self.revision
    }
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn cache_key(&self) -> String {
        blake3::hash(format!("{}\0{}", self.source, self.revision.to_ascii_lowercase()).as_bytes())
            .to_hex()
            .to_string()
    }

    pub fn object(&self) -> String {
        format!("{}:{}", self.revision.to_ascii_lowercase(), self.path)
    }
}

impl Manifest {
    pub fn load(path: &Path) -> Result<ValidatedManifest> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|_| Error::Invalid("manifest: cannot read the requested file".into()))?;
        if !metadata.is_file() || metadata.len() > MAX_MANIFEST as u64 {
            return Err(Error::Invalid(
                "manifest: require a regular file under 1 MiB".into(),
            ));
        }
        let bytes = crate::content::read_file(path, MAX_MANIFEST)
            .map_err(|_| Error::Invalid("manifest: cannot read the requested file".into()))?;
        parse::<Self>(&bytes, "manifest")?.validate()
    }
    pub fn validate(self) -> Result<ValidatedManifest> {
        let manifest = self;
        if manifest.skills.len() > 1000 || manifest.bundles.len() > 128 {
            return Err(Error::Invalid(
                "manifest: maximum count is 1000 skills and 128 bundles".into(),
            ));
        }
        if manifest.version != 1 {
            return Err(Error::Invalid(
                "version: only version 1 is supported".into(),
            ));
        }
        let project = targets(&manifest.targets, true)?;
        if project.contains(&Target::Hermes) {
            return Err(Error::Invalid(
                "targets: Hermes discovery support is not verified".into(),
            ));
        }
        let mut names = BTreeSet::new();
        for (index, skill) in manifest.skills.iter().enumerate() {
            skill
                .effective_targets(&project)
                .map_err(|error| Error::Invalid(format!("skills[{index}].{error}")))?;
            if !names.insert(&skill.name) {
                return Err(Error::Invalid(format!(
                    "skills[{index}].name: duplicate skill name"
                )));
            }
        }
        for (index, bundle) in manifest.bundles.iter().enumerate() {
            bundle
                .effective_targets(&project)
                .map_err(|error| Error::Invalid(format!("bundles[{index}].{error}")))?;
        }
        Ok(ValidatedManifest {
            manifest,
            targets: project,
        })
    }
}

impl Skill {
    pub fn effective_targets(&self, parent: &BTreeSet<Target>) -> Result<BTreeSet<Target>> {
        name(&self.name)?;
        if self.activation.as_deref().unwrap_or("on-demand") != "on-demand" {
            return Err(Error::Invalid(
                "activation: only on-demand is supported".into(),
            ));
        }
        narrowed(&self.targets, parent)
    }
}

impl BundleRef {
    pub fn effective_targets(&self, parent: &BTreeSet<Target>) -> Result<BTreeSet<Target>> {
        narrowed(&self.targets, parent)
    }
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub version: u32,
    pub skills: Vec<Skill>,
}

#[derive(Clone, Debug)]
pub struct ValidatedManifest {
    pub manifest: Manifest,
    pub targets: BTreeSet<Target>,
}

pub fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8], label: &str) -> Result<T> {
    if bytes.len() > MAX_MANIFEST {
        return Err(Error::Invalid(format!("{label}: maximum size is 1 MiB")));
    }
    // Parser errors can include credentials. Publish only the location.
    serde_yaml::from_slice(bytes).map_err(|error| {
        let line = error.location().map(|l| l.line()).unwrap_or(0);
        Error::Invalid(format!("{label}: invalid structure at line {line}"))
    })
}

pub fn name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || value.split('-').any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
        || matches!(value, "con" | "prn" | "aux" | "nul")
    {
        return Err(Error::Invalid(
            "name: use lowercase letters, digits, and single hyphens".into(),
        ));
    }
    Ok(())
}

pub fn relative_path(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 4096
        || value.split('/').count() > 64
        || value
            .chars()
            .any(|c| c.is_control() || "\\:*?[]<>|\"".contains(c))
        || value.split('/').any(|p| {
            matches!(p, "" | "." | "..")
                || p.eq_ignore_ascii_case(".git")
                || p.ends_with(['.', ' '])
                || p == ".env.local"
                || (p.starts_with(".env.") && p.ends_with(".local"))
                || p == ".sops-age-key.txt"
        })
    {
        return Err(Error::Invalid(
            "path: require a safe relative repository path".into(),
        ));
    }
    Ok(())
}

pub fn reference(source: &str, revision: &str, path: &str) -> Result<()> {
    let parsed = url::Url::parse(source)
        .map_err(|_| Error::Invalid("source: use a complete Git URL".into()))?;
    if !matches!(parsed.scheme(), "https" | "ssh")
        || parsed.host_str().is_none()
        || parsed.path().trim_matches('/').is_empty()
        || parsed.password().is_some()
        || (parsed.scheme() == "https" && !parsed.username().is_empty())
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || source.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(Error::Invalid(
            "source: use HTTPS or SSH without embedded credentials".into(),
        ));
    }
    if !matches!(revision.len(), 40 | 64) || !revision.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::Invalid(
            "revision: use a complete hexadecimal commit identifier".into(),
        ));
    }
    relative_path(path)
}

pub fn targets(values: &[Target], allow_all: bool) -> Result<BTreeSet<Target>> {
    if allow_all && values == [Target::All] {
        return Ok([
            Target::Codex,
            Target::Claude,
            Target::Pi,
            Target::Opencode,
            Target::Hermes,
        ]
        .into());
    }
    let result: BTreeSet<_> = values.iter().copied().collect();
    if result.is_empty() || result.len() != values.len() || result.contains(&Target::All) {
        return Err(Error::Invalid(
            "targets: require unique harnesses; all must stand alone".into(),
        ));
    }
    Ok(result)
}

pub fn narrowed(
    values: &Option<Vec<Target>>,
    parent: &BTreeSet<Target>,
) -> Result<BTreeSet<Target>> {
    let entry = match values {
        Some(values) => targets(values, false)?,
        None => parent.clone(),
    };
    if !entry.is_subset(parent) {
        return Err(Error::Invalid(
            "targets: an entry cannot widen parent targets".into(),
        ));
    }
    for target in [Target::Codex, Target::Pi, Target::Opencode] {
        if parent.contains(&target) && !entry.contains(&target) {
            return Err(Error::Invalid(
                "targets: canonical discovery cannot enforce this exclusion".into(),
            ));
        }
    }
    Ok(entry)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn rejects_unsafe_references() {
        for path in [
            "../x",
            "/x",
            "a//b",
            "a/./b",
            "C:/x",
            "a\\b",
            "a/*",
            ".git/config",
            ".env.local",
        ] {
            assert!(reference("https://github.com/example/skills", &"a".repeat(40), path).is_err());
        }
        for source in [
            "file:///tmp/repo",
            "https://secret@github.com/a/b",
            "https://github.com/a/b?token=x",
        ] {
            assert!(reference(source, &"a".repeat(40), "skills/x").is_err());
        }
        assert!(reference("ssh://git@github.com/a/b", &"a".repeat(64), "skills/x").is_ok());
    }

    #[test]
    fn target_policy_is_explicit() {
        let parent = targets(&[Target::Codex, Target::Claude], false).unwrap();
        assert!(narrowed(&Some(vec![Target::Claude]), &parent).is_err());
        assert!(narrowed(&Some(vec![Target::Codex]), &parent).is_ok());
        assert!(targets(&[Target::All, Target::Codex], true).is_err());
        assert!(targets(&[Target::Codex, Target::Codex], true).is_err());
    }

    #[test]
    fn parser_rejects_unknown_fields_without_leaking_values() {
        let error = parse::<Manifest>(
            b"version: 1\ntargets: [codex]\nskills: []\nsecret: sensitive-value",
            "manifest",
        )
        .unwrap_err();
        assert!(!error.to_string().contains("sensitive-value"));
        assert!(parse::<Bundle>(b"version: 1\nskills: []\nbundles: []", "bundle").is_err());
    }

    proptest::proptest! {
        #[test]
        fn accepted_paths_never_escape_root(value in ".{0,120}") {
            if relative_path(&value).is_ok() {
                proptest::prop_assert!(!Path::new(&value).is_absolute());
                proptest::prop_assert!(value.split('/').all(|part| !matches!(part, "" | "." | "..")));
                proptest::prop_assert!(!value.contains(['\\', ':', '\0']));
            }
        }
    }
}
