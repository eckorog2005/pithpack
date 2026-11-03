mod common;

use std::path::Path;

use assert_cmd::Command;
use common::{fixture, write};
use predicates::prelude::*;

/// Isolates the run from the developer's own global gitignore and git config.
fn pithpack(cwd: &Path, home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("pithpack").unwrap();
    cmd.current_dir(cwd)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env_remove("GIT_CONFIG_GLOBAL");
    cmd
}

#[test]
fn success_writes_only_the_bundle_to_stdout_and_the_report_to_stderr() {
    let dir = fixture(&[("src/lib.rs", "pub fn a() {}\n")]);
    let home = tempfile::tempdir().unwrap();

    pithpack(dir.path(), home.path())
        .arg("./src")
        .assert()
        .code(0)
        .stdout(
            predicate::str::starts_with("<bundle ").and(predicate::str::ends_with("</bundle>\n")),
        )
        .stderr(predicate::str::contains("src/lib.rs").and(predicate::str::contains(" bundle: ")));
}

#[test]
fn quiet_success_leaves_stderr_empty() {
    let dir = fixture(&[("src/lib.rs", "pub fn a() {}\n")]);
    let home = tempfile::tempdir().unwrap();

    pithpack(dir.path(), home.path())
        .args(["-q", "src"])
        .assert()
        .code(0)
        .stderr("");
}

#[test]
fn a_missing_path_exits_1_naming_it() {
    let dir = fixture(&[]);
    let home = tempfile::tempdir().unwrap();

    pithpack(dir.path(), home.path())
        .arg("nope/missing.rs")
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("nope/missing.rs"));
}

#[test]
fn a_usage_error_exits_2() {
    let dir = fixture(&[]);
    let home = tempfile::tempdir().unwrap();

    pithpack(dir.path(), home.path())
        .args([".", "--no-such-flag"])
        .assert()
        .code(2)
        .stdout("");
}

#[test]
fn the_global_gitignore_is_respected() {
    let dir = fixture(&[("keep.rs", ""), ("scratch.tmp", "")]);
    let home = tempfile::tempdir().unwrap();
    write(home.path(), ".config/git/ignore", "*.tmp\n");

    pithpack(dir.path(), home.path())
        .arg(".")
        .assert()
        .code(0)
        .stdout(
            predicate::str::contains("keep.rs").and(predicate::str::contains("scratch.tmp").not()),
        );
}

#[test]
fn the_same_inputs_produce_byte_identical_output() {
    let dir = fixture(&[
        ("b/x.rs", "x\n"),
        ("a/y.py", "y\n"),
        ("a/z/w.go", "w"),
        ("c.txt", "</file>"),
    ]);
    let home = tempfile::tempdir().unwrap();

    let first = pithpack(dir.path(), home.path())
        .args(["a", "b", "c.txt"])
        .output()
        .unwrap();
    let second = pithpack(dir.path(), home.path())
        .args(["a", "b", "c.txt"])
        .output()
        .unwrap();

    assert!(first.status.success());
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn an_invalid_max_file_size_is_a_usage_error() {
    let dir = fixture(&[("a.rs", "")]);
    let home = tempfile::tempdir().unwrap();

    for size in ["1GB", "lots", "-5", "1.5MiB"] {
        pithpack(dir.path(), home.path())
            .args([".", &format!("--max-file-size={size}")])
            .assert()
            .code(2)
            .stdout("")
            .stderr(predicate::str::contains("--max-file-size"));
    }
}

#[test]
fn piped_stdin_is_bundled_and_redacted_when_no_paths_are_given() {
    let dir = fixture(&[]);
    let home = tempfile::tempdir().unwrap();
    let aws = ["AKIA", "Z7QK2M4XW9TB3HNP"].concat();

    pithpack(dir.path(), home.path())
        .write_stdin(format!("+let key = \"{aws}\";\n"))
        .assert()
        .code(0)
        .stdout(
            predicate::str::contains("<file path=\"&lt;stdin&gt;\" lang=\"text\" ")
                .and(predicate::str::contains(
                    "+let key = \"[REDACTED:aws-access-key]\";\n",
                ))
                .and(predicate::str::contains(aws.as_str()).not()),
        )
        .stderr(
            predicate::str::contains(" <stdin> ").and(predicate::str::contains(aws.as_str()).not()),
        );
}

#[test]
fn an_unknown_lang_is_a_usage_error() {
    let dir = fixture(&[]);
    let home = tempfile::tempdir().unwrap();

    pithpack(dir.path(), home.path())
        .args(["--lang", "cobol"])
        .write_stdin("x\n")
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicate::str::contains("--lang"));
}

#[test]
fn every_mode_flag_compresses_as_strip_and_is_labelled_so() {
    let dir = fixture(&[("a.rs", "// c\npub fn a() {}\n")]);
    let home = tempfile::tempdir().unwrap();

    for flags in [
        &["-m", "strip"][..],
        &["--mode", "skeleton"],
        &["-m", "outline"],
        &["-0"],
        &["-1"],
        &["-2"],
        &["--no-docs"],
        &["-2", "--no-docs"],
    ] {
        pithpack(dir.path(), home.path())
            .args(flags)
            .arg("a.rs")
            .assert()
            .code(0)
            .stdout(
                predicate::str::contains(" mode=\"strip\" tokens=\"")
                    .and(predicate::str::contains("// c").not())
                    .and(predicate::str::contains("mode=\"skeleton\"").not())
                    .and(predicate::str::contains("mode=\"outline\"").not()),
            )
            .stderr(predicate::str::contains(" strip").and(predicate::str::contains("0.0%").not()));
    }
}

#[test]
fn conflicting_mode_flags_are_a_usage_error() {
    let dir = fixture(&[("a.rs", "pub fn a() {}\n")]);
    let home = tempfile::tempdir().unwrap();

    for flags in [
        &["-0", "-1"][..],
        &["-1", "-m", "outline"],
        &["-m", "strip", "-2"],
    ] {
        pithpack(dir.path(), home.path())
            .args(flags)
            .arg("a.rs")
            .assert()
            .code(2)
            .stdout("");
    }
}

#[test]
fn an_unknown_mode_is_a_usage_error() {
    let dir = fixture(&[("a.rs", "pub fn a() {}\n")]);
    let home = tempfile::tempdir().unwrap();

    pithpack(dir.path(), home.path())
        .args(["-m", "huge", "a.rs"])
        .assert()
        .code(2);
}

#[test]
fn init_creates_pithpack_toml_then_exits_1_without_overwriting() {
    let dir = fixture(&[]);
    let home = tempfile::tempdir().unwrap();

    pithpack(dir.path(), home.path())
        .arg("init")
        .assert()
        .code(0);
    let created = std::fs::read_to_string(dir.path().join("pithpack.toml")).unwrap();

    std::fs::write(dir.path().join("pithpack.toml"), "tree = false\n").unwrap();
    pithpack(dir.path(), home.path())
        .arg("init")
        .assert()
        .code(1);
    assert!(created.contains("mode"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("pithpack.toml")).unwrap(),
        "tree = false\n"
    );
}
