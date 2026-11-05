#[path = "../benches/gen_fixture.rs"]
#[allow(dead_code)]
mod gen_fixture;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                files.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    files
}

#[test]
fn two_runs_produce_byte_identical_trees() {
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    gen_fixture::generate(a.path()).unwrap();
    gen_fixture::generate(b.path()).unwrap();
    assert!(tree(a.path()) == tree(b.path()));
}

#[test]
fn workload_matches_sdd_shape() {
    let dir = tempfile::tempdir().unwrap();
    gen_fixture::generate(dir.path()).unwrap();
    let files = tree(dir.path());
    assert_eq!(files.len(), 1000);
    for ext in [".rs", ".py", ".ts", ".go"] {
        assert_eq!(files.keys().filter(|k| k.ends_with(ext)).count(), 250);
    }
    let avg = files.values().map(Vec::len).sum::<usize>() / files.len();
    assert!((7 * 1024..=9 * 1024).contains(&avg), "average {avg}");
}
