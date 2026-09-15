use agents_manifest::{
    content::{FileContent, Tree},
    manifest::{self, Manifest},
};
use std::sync::Arc;

fn main() {
    divan::main();
}

#[divan::bench]
fn manifest_validation(bencher: divan::Bencher) {
    let bytes = include_bytes!("../examples/skills.yaml");
    bencher.bench(|| {
        manifest::parse::<Manifest>(divan::black_box(bytes), "benchmark")
            .and_then(Manifest::validate)
    });
}

#[divan::bench(args = [1, 16, 128])]
fn tree_fingerprint(bencher: divan::Bencher, count: usize) {
    let mut tree = Tree::new();
    let bytes: Arc<[u8]> = Arc::from(vec![b'x'; 4096]);
    for index in 0..count {
        tree.insert(
            format!("references/file-{index}.md"),
            FileContent {
                bytes: bytes.clone(),
                executable: false,
            },
        );
    }
    bencher.bench(|| divan::black_box(&tree).fingerprint());
}
