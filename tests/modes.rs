mod common;

use common::{fixture, run};
use pithpack::cli::ModeArg;
use pithpack::compressor::compress;
use pithpack::parser::{self, Language};

const RUST: &str = "\
//! Crate docs survive.
use std::fmt;

/// A point.
pub struct Point {
    pub x: i32,
    pub y: i32,
}

pub enum Shape {
    Dot(Point),
    Line { from: Point, to: Point },
}

pub trait Area {
    fn area(&self) -> f64;

    /// Default bodies collapse too.
    fn doubled(&self) -> f64 {
        let a = self.area();
        a * 2.0
    }
}

impl Point {
    /// Kept on one line.
    pub fn x(&self) -> i32 { self.x }

    // A Free Comment inside a container.
    pub fn sum(&self) -> i32 {
        fn nested() -> i32 {
            0
        }
        let s = \"multi   \n\n\nline\";
        self.x + self.y + nested() + s.len() as i32
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    where
        Self: Sized,
    {
        write!(f, \"({}, {})\", self.x, self.y)
    }
}

const ORIGIN: Point = Point { x: 0, y: 0 };

fn empty() {}
";

/// Every `skeleton`/`outline` output for a Full-support language must re-parse cleanly.
fn assert_reparses(lang: Language, text: &str) {
    let tree = parser::parse(lang, text).unwrap();
    assert!(!tree.root_node().has_error(), "{text}");
}

#[test]
fn rust_skeleton_collapses_multi_line_function_bodies_only() {
    let out = compress(Some(Language::Rust), RUST, ModeArg::Skeleton, true);

    assert_eq!(out.mode, ModeArg::Skeleton);
    assert_eq!(out.fallback, None);
    assert_reparses(Language::Rust, &out.text);
    insta::assert_snapshot!(out.text);
}

#[test]
fn rust_skeleton_with_no_docs_still_reparses() {
    let out = compress(Some(Language::Rust), RUST, ModeArg::Skeleton, false);

    assert!(!out.text.contains("///"), "{}", out.text);
    assert_reparses(Language::Rust, &out.text);
}

#[test]
fn rust_outline_gets_skeleton_until_built() {
    let out = compress(Some(Language::Rust), RUST, ModeArg::Outline, true);

    assert_eq!(out.mode, ModeArg::Skeleton);
    assert_eq!(out.fallback.map(|f| f.reason()), Some("mode_not_built"));
    assert_reparses(Language::Rust, &out.text);
}

#[test]
fn a_badly_broken_rust_file_falls_back_to_strip_in_skeleton() {
    let source = "fn ( { ] let = ;;; ) impl\nfn a() {\n    1\n}\n";

    let out = compress(Some(Language::Rust), source, ModeArg::Skeleton, true);

    assert_eq!(out.mode, ModeArg::Strip);
    assert_eq!(out.fallback.map(|f| f.reason()), Some("parse_errors"));
    assert!(out.text.contains("    1\n"), "{}", out.text);
}

#[test]
fn the_default_run_is_skeleton_and_labelled_so() {
    let dir = fixture(&[("lib.rs", "fn a() {\n    1\n}\n")]);

    let out = run(dir.path(), &["--no-tree", "lib.rs"]);

    assert!(
        out.stdout.contains(" mode=\"skeleton\" tokenizer="),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(
            "<file path=\"lib.rs\" lang=\"rust\" mode=\"skeleton\" tokens=\"8\">\nfn a() { /* … */ }\n</file>"
        ),
        "{}",
        out.stdout
    );
    assert!(
        out.stderr.contains(" lib.rs  rust  skeleton  "),
        "{}",
        out.stderr
    );
}
