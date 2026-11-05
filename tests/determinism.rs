mod common;

use std::path::Path;

use assert_cmd::Command;
use common::{fixture, write};

fn fixture_tree() -> tempfile::TempDir {
    let dir = fixture(&[]);
    for i in 0..60 {
        let sub = format!("d{}/e{}", i % 7, i % 3);
        write(
            dir.path(),
            &format!("{sub}/f{i}.rs"),
            &format!("/// doc {i}\npub fn f{i}() -> u32 {{\n    let x = {i};\n    x + 1\n}}\n"),
        );
        write(
            dir.path(),
            &format!("{sub}/g{i}.py"),
            &format!("def g{i}():\n    return {i}\n"),
        );
    }
    write(dir.path(), "secret.txt", "key = AKIAIOSFODNN7EXAMPLE\n");
    write(dir.path(), "blob.bin", "\0\0\0");
    dir
}

fn run(cwd: &Path, threads: &str, args: &[&str]) -> (Vec<u8>, String) {
    let home = tempfile::tempdir().unwrap();
    let out = Command::cargo_bin("pithpack")
        .unwrap()
        .current_dir(cwd)
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", home.path().join(".config"))
        .env("RAYON_NUM_THREADS", threads)
        .args(["--no-config", "--report", "json", "."])
        .args(args)
        .output()
        .unwrap();
    let report = String::from_utf8(out.stderr).unwrap();
    let report = report
        .lines()
        .filter(|l| !l.contains("elapsed_ms"))
        .collect::<Vec<_>>()
        .join("\n");
    (out.stdout, report)
}

#[test]
fn output_is_identical_across_runs_thread_counts_modes_and_formats() {
    let dir = fixture_tree();
    for mode in ["-0", "-1", "-2"] {
        for format in ["xml", "markdown"] {
            let args = [mode, "--format", format];
            let baseline = run(dir.path(), "1", &args);
            assert!(!baseline.0.is_empty());
            for threads in ["2", "8", "8"] {
                assert_eq!(
                    run(dir.path(), threads, &args),
                    baseline,
                    "{args:?} x{threads}"
                );
            }
        }
    }
}
