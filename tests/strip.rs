mod common;

use common::{fixture, run};

const RUST: &str = "\
//! Crate docs survive.

// A free header comment.
use std::fmt;

/// Outer docs survive.
/** Block docs survive. */
pub struct Point {
    pub x: i32, // trailing note
    /* inline */ pub y: i32,
}



impl Point {
    //// Four slashes is a Free Comment.
    pub fn sum(&self) -> i32 {
        /*
         * A multi-line Free Comment.
         */
        let total/*glued*/= self.x + self.y;
        let s = \"// not a comment\";
        total
    }
}

/**/
fn helper() {}
";

/// Drops the header line, whose version attribute would churn snapshots on every release.
fn files(stdout: &str) -> &str {
    stdout.split_once('\n').unwrap().1
}

#[test]
fn strip_removes_free_comments_and_keeps_doc_comments_in_rust() {
    let dir = fixture(&[("lib.rs", RUST)]);

    let out = run(
        dir.path(),
        &["-0", "--no-tree", "--report", "none", "lib.rs"],
    );

    insta::assert_snapshot!(files(&out.stdout));
}

#[test]
fn no_docs_also_removes_doc_comments() {
    let dir = fixture(&[("lib.rs", RUST)]);

    let out = run(
        dir.path(),
        &["-0", "--no-docs", "--no-tree", "--report", "none", "lib.rs"],
    );

    insta::assert_snapshot!(files(&out.stdout));
}

#[test]
fn a_badly_broken_rust_file_falls_back_to_strip_and_the_report_says_why() {
    let dir = fixture(&[
        ("broken.rs", "fn ( { ] let = ;;; ) impl\n// gone\n"),
        ("ok.rs", "fn ok() {}\n"),
    ]);

    let out = run(dir.path(), &["-0", "broken.rs", "ok.rs"]);

    assert!(
        out.stdout
            .contains("<file path=\"broken.rs\" lang=\"rust\" mode=\"strip\"")
    );
    assert!(!out.stdout.contains("// gone"), "{}", out.stdout);
    assert!(
        out.stderr.contains(" broken.rs  rust  strip*  "),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.contains(" ok.rs      rust  strip   "),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.ends_with(" * over 10% parse errors\n"),
        "{}",
        out.stderr
    );
}

#[test]
fn the_json_report_gives_the_fallback_reason_and_error_ratio() {
    let dir = fixture(&[("broken.rs", "fn ( { ] let = ;;; ) impl\n")]);

    let out = run(dir.path(), &["-0", "--report", "json", "broken.rs"]);

    let json: serde_json::Value = serde_json::from_str(&out.stderr).unwrap();
    let file = &json["files"][0];
    assert_eq!(file["effective_mode"], "strip");
    assert_eq!(file["fallback"]["reason"], "parse_errors");
    assert!(file["fallback"]["error_ratio"].as_f64().unwrap() > 0.1);
}

#[test]
fn plain_text_is_only_trimmed_and_blank_line_collapsed() {
    let dir = fixture(&[("notes.txt", "\n\ntitle   \n\n\n\n  // kept, indented\t\n\n")]);

    let out = run(dir.path(), &["--no-tree", "notes.txt"]);

    assert!(
        out.stdout.contains(
            "lang=\"text\" mode=\"strip\" tokens=\"9\">\n\
             title\n\n  // kept, indented\n</file>\n"
        ),
        "{}",
        out.stdout
    );
    assert!(
        out.stderr.contains(" notes.txt  text  strip*  "),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.contains(" * Plain Text: whitespace only\n"),
        "{}",
        out.stderr
    );
}

#[test]
fn stdin_is_compressed_only_when_its_language_is_given() {
    let dir = fixture(&[]);
    let source = "fn a() {} // free\n";

    let plain = common::run_with_stdin(dir.path(), &["-0"], source);
    let rust = common::run_with_stdin(dir.path(), &["-0", "--lang", "rust"], source);

    assert!(
        plain.stdout.contains("fn a() {} // free\n"),
        "{}",
        plain.stdout
    );
    assert!(
        rust.stdout.contains("fn a() {}\n</file>"),
        "{}",
        rust.stdout
    );
    assert!(
        rust.stderr.contains(" <stdin>  rust  strip  18B → 10B"),
        "{}",
        rust.stderr
    );
}

#[test]
fn skeleton_and_outline_are_labelled_as_the_strip_they_get_until_built() {
    let dir = fixture(&[("lib.rs", "fn a() {} // free\n")]);

    for flag in ["-1", "-2"] {
        let out = run(dir.path(), &["--no-tree", flag, "lib.rs"]);

        assert!(
            out.stdout
                .starts_with("<bundle tool=\"pithpack\" version=\""),
            "{}",
            out.stdout
        );
        assert!(
            out.stdout.contains(" mode=\"strip\" tokenizer="),
            "{}",
            out.stdout
        );
        assert!(
            out.stdout
                .contains("<file path=\"lib.rs\" lang=\"rust\" mode=\"strip\"")
        );
        assert!(
            out.stderr.contains(" lib.rs  rust  strip*  "),
            "{}",
            out.stderr
        );
        assert!(
            out.stderr
                .contains(" * skeleton and outline are not built yet\n"),
            "{}",
            out.stderr
        );
    }
}

#[test]
fn the_budget_is_checked_against_the_compressed_bundle() {
    let comments = "// padding that strip removes\n".repeat(200);
    let dir = fixture(&[("lib.rs", &format!("{comments}fn a() {{}}\n"))]);

    let out = run(dir.path(), &["-0", "--budget", "200", "lib.rs"]);

    assert_eq!(
        out.outcome.unwrap(),
        pithpack::Outcome::Ok,
        "{}",
        out.stderr
    );
}

#[test]
fn whitespace_inside_multi_line_string_literals_is_kept() {
    let source = "fn a() {\n    let s = \"x   \n\n\n\ny\";   \n    let r = r#\"z  \n\"#;\n}\n";
    let dir = fixture(&[("lib.rs", source)]);

    let out = run(dir.path(), &["-0", "--no-tree", "lib.rs"]);

    assert!(
        out.stdout.contains(
            "fn a() {\n    let s = \"x   \n\n\n\ny\";\n    let r = r#\"z  \n\"#;\n}\n</file>"
        ),
        "{}",
        out.stdout
    );
}
