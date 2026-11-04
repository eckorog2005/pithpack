mod common;

use std::io::Write;
use std::process::{Command, Stdio};

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
fn go_skeleton_and_outline_are_labelled_as_the_strip_they_get_until_built() {
    let dir = fixture(&[("main.go", "package main // free\n")]);

    for flag in ["-1", "-2"] {
        let out = run(dir.path(), &["--no-tree", flag, "main.go"]);

        assert!(
            out.stdout
                .starts_with("<bundle tool=\"pithpack\" version=\""),
            "{}",
            out.stdout
        );
        assert!(
            out.stdout.contains(" mode=\"skeleton\" tokenizer="),
            "{}",
            out.stdout
        );
        assert!(
            out.stdout
                .contains("<file path=\"main.go\" lang=\"go\" mode=\"strip\"")
        );
        assert!(
            out.stderr.contains(" main.go  go    strip*  "),
            "{}",
            out.stderr
        );
        assert!(
            out.stderr
                .contains(" * Mode not built for this language yet\n"),
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

const PYTHON: &str = "\
#!/usr/bin/env python3
\"\"\"Module docs survive.\"\"\"

# A free header comment.
import os


class Point:
    \"\"\"Class docs survive.

    Even    their    whitespace.
    \"\"\"

    x = 1  # trailing note

    def total(self):
        '''Function docs survive.'''
        # A free comment in a body.
        s = \"# not a comment\"
        note = \"\"\"not a docstring\"\"\"
        return self.x


def only_docs():
    \"\"\"The whole body.\"\"\"


def nested():
    if True:
        \"\"\"An if body has no docstring.\"\"\"
        return 1
";

/// The compressed text of the one `<file>` block in a Bundle.
fn file_text(stdout: &str) -> &str {
    let file = &stdout[stdout.find("<file ").unwrap()..];
    let start = file.find(">\n").unwrap() + 2;
    &file[start..file.rfind("</file>").unwrap()]
}

/// Asks a real Python, since tree-sitter's error recovery accepts some invalid code (an empty body).
fn assert_valid_python(source: &str) {
    let check = "import ast, sys; ast.parse(sys.stdin.read())";
    let output = ["python3", "python"]
        .into_iter()
        .find_map(|python| {
            let mut child = Command::new(python)
                .args(["-c", check])
                .stdin(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .ok()?;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(source.as_bytes())
                .unwrap();
            Some(child.wait_with_output().unwrap())
        })
        .expect("a Python interpreter on PATH");
    assert!(
        output.status.success(),
        "{}\n{source}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn strip_removes_free_comments_and_keeps_docstrings_in_python() {
    let dir = fixture(&[("app.py", PYTHON)]);

    let out = run(
        dir.path(),
        &["-0", "--no-tree", "--report", "none", "app.py"],
    );

    insta::assert_snapshot!(files(&out.stdout));
    assert_valid_python(file_text(&out.stdout));
}

#[test]
fn no_docs_removes_python_docstrings_and_keeps_emptied_bodies_valid() {
    let dir = fixture(&[("app.py", PYTHON)]);

    let out = run(
        dir.path(),
        &["-0", "--no-docs", "--no-tree", "--report", "none", "app.py"],
    );

    insta::assert_snapshot!(files(&out.stdout));
    assert_valid_python(file_text(&out.stdout));
}

#[test]
fn no_docs_leaves_valid_python_and_only_removes_plain_string_docstrings() {
    let source =
        "\"\"\"Doc.\"\"\"; x = 1\n\ndef f():\n    f\"\"\"{x}\"\"\"\n\ndef g():\n    b\"bytes\"\n";
    let dir = fixture(&[("app.py", source)]);

    let out = run(
        dir.path(),
        &["-0", "--no-docs", "--no-tree", "--report", "none", "app.py"],
    );

    assert_eq!(
        file_text(&out.stdout),
        "x = 1\n\ndef f():\n    f\"\"\"{x}\"\"\"\n\ndef g():\n    b\"bytes\"\n"
    );
    assert_valid_python(file_text(&out.stdout));
}

const GO: &str = "\
// Package docs survive.
package geo

// A free comment before imports.
import \"fmt\"

// Point docs survive,
// all of the block.
type Point struct {
\t// X docs survive.
\tX int // trailing note
\t// Y docs survive.
\t/* so does this */
\tY int

\t// Detached from Z by a blank line.

\tZ int
}

/* Sum docs survive. */
func (p Point) Sum() int {
\t// A free comment in a body.
\ts := `// not a comment
\t   kept   `
\treturn p.X + p.Y + len(s)
}

// Detached from Helper.

func Helper() { fmt.Println() }

const (
\t// A docs survive.
\tA = 1
)

var (
\t// B docs survive.
\tB, C = 2, 3
)

type (
\t// Shape docs survive.
\tShape interface {
\t\t// Area docs survive.
\t\tArea() float64
\t}
)

// Local docs survive.
func Local() {
\t// A local const is not a declaration doc.
\tconst d = 1
}
";

#[test]
fn strip_removes_free_comments_and_keeps_doc_comments_in_go() {
    let dir = fixture(&[("geo.go", GO)]);

    let out = run(
        dir.path(),
        &["-0", "--no-tree", "--report", "none", "geo.go"],
    );

    insta::assert_snapshot!(files(&out.stdout));
}

#[test]
fn no_docs_removes_go_doc_comments() {
    let dir = fixture(&[("geo.go", GO)]);

    let out = run(
        dir.path(),
        &["-0", "--no-docs", "--no-tree", "--report", "none", "geo.go"],
    );

    insta::assert_snapshot!(files(&out.stdout));
}

const TYPESCRIPT: &str = "\
/** Module header is not before a declaration. */

// A free header comment.
import { a } from \"./a\";

/** Shape docs survive. */
export interface Shape {
  /** Area docs survive. */
  area(): number; // trailing note
}

/* A block comment is never a Doc Comment. */
type Id = string;

enum Color {
  /** Red docs survive. */
  Red,
  /** Blue docs survive. */
  Blue = 2,
}

/** Point docs survive. */
@sealed
class Point {
  /** X docs survive. */
  x = 1;

  /** Sum docs survive. */
  sum(): number {
    /** Not before a declaration. */
    a();
    const s = `// not a comment
      kept   `;
    return this.x/*glued*/+s.length;
  }
}

/** Detached from helper by a blank line. */

function helper() {}

/** Total docs survive. */
const total = 1;
";

#[test]
fn strip_removes_free_comments_and_keeps_doc_comments_in_typescript() {
    let dir = fixture(&[("shape.ts", TYPESCRIPT)]);

    let out = run(
        dir.path(),
        &["-0", "--no-tree", "--report", "none", "shape.ts"],
    );

    insta::assert_snapshot!(files(&out.stdout));
}

const JSX: &str = "\
// A free header comment.
import React from \"react\";

/** Card docs survive. */
export function Card({ url }) {
  // A free comment in a body.
  return (
    <a href=\"http://example.com\" title=\"/* not a comment */\">
      {/* A JSX comment. */}
      Visit   // not a comment either
      <b>{url}</b>
    </a>
  );
}
";

#[test]
fn strip_leaves_jsx_markup_untouched() {
    let dir = fixture(&[("card.jsx", JSX)]);

    let out = run(
        dir.path(),
        &["-0", "--no-tree", "--report", "none", "card.jsx"],
    );

    insta::assert_snapshot!(files(&out.stdout));
}

#[test]
fn strip_leaves_tsx_markup_untouched() {
    let tsx = JSX.replace("({ url })", "({ url }: { url: string })");
    let dir = fixture(&[("card.tsx", &tsx)]);

    let out = run(
        dir.path(),
        &["-0", "--no-tree", "--report", "none", "card.tsx"],
    );

    insta::assert_snapshot!(files(&out.stdout));
}

#[test]
fn no_docs_removes_jsdoc_comments() {
    let dir = fixture(&[("shape.ts", TYPESCRIPT)]);

    let out = run(
        dir.path(),
        &[
            "-0",
            "--no-docs",
            "--no-tree",
            "--report",
            "none",
            "shape.ts",
        ],
    );

    insta::assert_snapshot!(files(&out.stdout));
}

const JAVASCRIPT: &str = "\
/** Exports docs survive. */
module.exports.area = function () {};

/** Lint docs survive. */
// eslint-disable-next-line
function lint() {}

const point = {
  /** Prop docs survive. */
  x: 1, // trailing note
};

/** Not before a declaration. */
lint();
";

#[test]
fn strip_removes_free_comments_and_keeps_doc_comments_in_javascript() {
    let dir = fixture(&[("geo.js", JAVASCRIPT)]);

    let out = run(
        dir.path(),
        &["-0", "--no-tree", "--report", "none", "geo.js"],
    );

    insta::assert_snapshot!(files(&out.stdout));
}

const HTML: &str = "\
<!DOCTYPE html>
<!-- A free header comment. -->
<html>
<head>
  <title>A <!-- literal --> title</title>
  <!--[if IE]><link rel=\"stylesheet\" href=\"ie.css\"><![endif]-->
  <style>
    /* style comments are CSS, left as is */
    p { color: red; }
  </style>
  <script>
    // script comments are JavaScript, left as is
    const s = \"<!-- not a comment -->\";
  </script>
</head>
<body>
  <!--[if !IE]><!--><p>Not IE</p><!--<![endif]-->
  <p title=\"<!-- not a comment -->\">foo<!-- inline -->bar</p>
  <img alt='<!-- not a comment -->' />
  <textarea><!-- literal text --></textarea>
  <!--
    A multi-line comment.
  -->
</body>
</html>
";

#[test]
fn strip_removes_html_comments_and_leaves_script_and_style_untouched() {
    let dir = fixture(&[("index.html", HTML)]);

    let out = run(
        dir.path(),
        &["-0", "--no-tree", "--report", "none", "index.html"],
    );

    insta::assert_snapshot!(files(&out.stdout));
}

const CSS: &str = "\
/* A free header comment. */
@import url(\"//example.com/a.css\");

/**
 * Not a Doc Comment: CSS has none.
 */
.card {
  color: red; /* trailing note */
  content: \"/* not a comment */\";
  background: url(http://example.com/x.png);
  margin:/*glued*/0;
}
";

#[test]
fn strip_removes_every_css_comment_even_without_docs() {
    let dir = fixture(&[("site.css", CSS)]);

    let docs = run(
        dir.path(),
        &["-0", "--no-tree", "--report", "none", "site.css"],
    );
    let no_docs = run(
        dir.path(),
        &[
            "-0",
            "--no-docs",
            "--no-tree",
            "--report",
            "none",
            "site.css",
        ],
    );

    assert_eq!(docs.stdout, no_docs.stdout);
    insta::assert_snapshot!(files(&docs.stdout));
}
