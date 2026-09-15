#![allow(clippy::unwrap_used, clippy::expect_used)]

use flate2::{Compression, write::ZlibEncoder};
use sha1::{Digest, Sha1};
use std::{collections::BTreeMap, fs, io::Write, path::PathBuf, process::Command};

pub struct FixtureRepository {
    directory: tempfile::TempDir,
}

impl FixtureRepository {
    pub fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .current_dir(directory.path())
                .args(["init", "--bare", "--quiet", "--object-format=sha1"])
                .status()
                .unwrap()
                .success()
        );
        Self { directory }
    }

    pub fn path(&self) -> PathBuf {
        self.directory.path().to_owned()
    }

    pub fn object(&self, kind: &str, body: &[u8]) -> String {
        // Fixture objects are test data. No commit or credentialed Git command runs.
        let mut raw = format!("{kind} {}\0", body.len()).into_bytes();
        raw.extend_from_slice(body);
        let oid = format!("{:x}", Sha1::digest(&raw));
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&raw).unwrap();
        let bytes = encoder.finish().unwrap();
        let directory = self.directory.path().join("objects").join(&oid[..2]);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(&oid[2..]), bytes).unwrap();
        oid
    }

    pub fn tree(&self, entries: &[(&str, &str, &str)]) -> String {
        let sorted: BTreeMap<_, _> = entries
            .iter()
            .map(|(mode, name, oid)| (*name, (*mode, *oid)))
            .collect();
        let mut body = Vec::new();
        for (name, (mode, oid)) in sorted {
            body.extend_from_slice(format!("{mode} {name}\0").as_bytes());
            for pair in oid.as_bytes().as_chunks::<2>().0 {
                body.push(u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap());
            }
        }
        self.object("tree", &body)
    }

    pub fn commit(&self, tree: &str) -> String {
        let text = format!(
            "tree {tree}\nauthor Fixture <fixture@example.test> 0 +0000\ncommitter Fixture <fixture@example.test> 0 +0000\n\nFixture\n"
        );
        let commit = self.object("commit", text.as_bytes());
        fs::write(
            self.directory.path().join("FETCH_HEAD"),
            format!("{commit}\n"),
        )
        .unwrap();
        commit
    }
}
