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

const PYTHON: &str = r#"
"""Module docstring survives."""
import os
from typing import List


class Shape:
    """A shape."""

    sides = 0

    def __init__(self, name):
        """Build it."""
        self.name = name
        # A Free Comment in a body.
        self.size = 0

    def area(self):
        return 0

    @property
    def label(self):
        text = """multi   

line"""
        return text.strip()

    def only_doc(self):
        """Nothing else."""


async def fetch(url: str) -> List[str]:
    def inner():
        return 1
    return [url, str(inner())]


def one_liner(): return os.sep
"#;

const GO: &str = r#"// Package shapes has shapes.
package shapes

import "fmt"

// Shape is a shape.
type Shape struct {
	Name string
}

type Namer interface {
	Name() string
}

// Area returns the area.
func (s Shape) Area() int {
	// A Free Comment in a body.
	size := 2
	return size * size
}

func Describe(s Shape) string {
	text := `multi
line`
	f := func() int {
		return 1
	}
	return fmt.Sprint(text, f())
}

func Short() int { return 0 }
"#;

const TYPESCRIPT: &str = r#"
import { readFile } from "fs";

/** A shape. */
export interface Shape {
  area(): number;
}

export enum Color { Red, Green }

export type Id = string | number;

/** Base class. */
export abstract class Base implements Shape {
  /** The name. */
  name: string = "base";

  abstract area(): number;

  /** Build it. */
  constructor(name: string) {
    // A Free Comment in a body.
    this.name = name;
    this.touch();
  }

  describe(): string {
    return `multi
line ${this.name}`;
  }

  quick(): number { return 1; }
}

/** Fetch it. */
export async function fetchAll(urls: string[]): Promise<string[]> {
  const inner = () => {
    return urls.length;
  };
  const arrow = (x: number) => x + 1;
  return urls.map((u) => {
    return u + inner() + arrow(1);
  });
}
"#;

const TSX: &str = r#"
import React from "react";

/** Props. */
interface Props {
  label: string;
}

/** A button. */
export function Button({ label }: Props) {
  const handle = () => {
    console.log(label);
    return 1;
  };
  return (
    <button onClick={handle}>
      {/* A Free Comment in markup. */}
      {label}
    </button>
  );
}
"#;

const JAVASCRIPT: &str = r#"
const fs = require("fs");

/** A thing. */
class Thing {
  /** Build it. */
  constructor(name) {
    // A Free Comment in a body.
    this.name = name;
    this.size = 0;
  }

  get label() {
    return this.name;
  }

  static make() {
    return new Thing("x");
  }
}

/** Run it. */
function run(items) {
  const doubled = items.map(function (item) {
    return item * 2;
  });
  return doubled;
}

const short = (a) => a + 1;

const wrapped = (a) => ({
  a,
  b: 2,
});

module.exports = { Thing, run, short };
"#;

const JSX: &str = r#"
import React from "react";

/** A card. */
export const Card = ({ title }) => {
  const upper = title.toUpperCase();
  return (
    <div>
      {/* A Free Comment in markup. */}
      <h1>{upper}</h1>
    </div>
  );
};
"#;

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
fn python_skeleton_collapses_bodies_keeping_docstrings_first() {
    let out = compress(Some(Language::Python), PYTHON, ModeArg::Skeleton, true);

    assert_eq!(out.mode, ModeArg::Skeleton);
    assert_eq!(out.fallback, None);
    assert_reparses(Language::Python, &out.text);
    insta::assert_snapshot!(out.text);
}

#[test]
fn python_skeleton_with_no_docs_leaves_bare_ellipsis_bodies() {
    let out = compress(Some(Language::Python), PYTHON, ModeArg::Skeleton, false);

    assert!(!out.text.contains("Build it"), "{}", out.text);
    assert!(
        out.text.contains("    def only_doc(self):\n        ...\n"),
        "{}",
        out.text
    );
    assert!(
        out.text
            .contains("    def __init__(self, name):\n        ...\n"),
        "{}",
        out.text
    );
    assert_reparses(Language::Python, &out.text);
}

#[test]
fn go_skeleton_collapses_function_and_method_bodies_keeping_types_and_docs() {
    let out = compress(Some(Language::Go), GO, ModeArg::Skeleton, true);

    assert_eq!(out.mode, ModeArg::Skeleton);
    assert_eq!(out.fallback, None);
    assert_reparses(Language::Go, &out.text);
    insta::assert_snapshot!(out.text);
}

#[test]
fn go_skeleton_with_no_docs_still_reparses() {
    let out = compress(Some(Language::Go), GO, ModeArg::Skeleton, false);

    assert!(!out.text.contains("Area returns"), "{}", out.text);
    assert_reparses(Language::Go, &out.text);
}

#[test]
fn ts_and_js_skeletons_collapse_bodies_keeping_declarations_and_jsdoc() {
    for (name, lang, source) in [
        ("typescript", Language::TypeScript, TYPESCRIPT),
        ("tsx", Language::Tsx, TSX),
        ("javascript", Language::JavaScript, JAVASCRIPT),
        ("jsx", Language::Jsx, JSX),
    ] {
        let out = compress(Some(lang), source, ModeArg::Skeleton, true);

        assert_eq!(out.mode, ModeArg::Skeleton, "{name}");
        assert_eq!(out.fallback, None, "{name}");
        assert_reparses(lang, &out.text);
        insta::assert_snapshot!(format!("{name}_skeleton"), out.text);
    }
}

#[test]
fn ts_and_js_skeletons_with_no_docs_still_reparse() {
    for (lang, source) in [
        (Language::TypeScript, TYPESCRIPT),
        (Language::Tsx, TSX),
        (Language::JavaScript, JAVASCRIPT),
        (Language::Jsx, JSX),
    ] {
        let out = compress(Some(lang), source, ModeArg::Skeleton, false);

        assert!(!out.text.contains("/**"), "{}", out.text);
        assert_reparses(lang, &out.text);
    }
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

fn file_blocks(bundle: &str) -> &str {
    let start = bundle.find("<file ").unwrap();
    &bundle[start..bundle.rfind("</bundle>").unwrap()]
}

#[test]
fn html_and_css_fall_back_to_strip_with_a_strip_only_note() {
    let dir = fixture(&[
        ("a.html", "<!-- c -->\n<p>hi</p>\n"),
        ("b.css", "/* c */\na { color: red; }\n"),
    ]);

    for name in ["a.html", "b.css"] {
        let strip = run(dir.path(), &["-0", "--no-tree", name]).stdout;
        for flag in ["-1", "-2"] {
            let out = run(dir.path(), &[flag, "--no-tree", "--report", "json", name]);
            let json: serde_json::Value = serde_json::from_str(&out.stderr).unwrap();
            let file = &json["files"][0];
            assert_eq!(file["effective_mode"], "strip", "{name} {flag}");
            assert_eq!(file["fallback"]["reason"], "strip_only", "{name} {flag}");
            assert_eq!(
                file_blocks(&out.stdout),
                file_blocks(&strip),
                "{name} {flag}"
            );
        }
        let table = run(dir.path(), &["-1", "--no-tree", name]).stderr;
        assert!(
            table.ends_with(" * strip is the only Mode for this language\n"),
            "{table}"
        );
    }
}

const RUST_OUTLINE: &str = "\
//! Crate docs survive.
#![allow(dead_code)]

use std::fmt;
pub use std::io::Read;
pub(crate) use std::io::Write;

/// Public.
#[derive(Debug)]
pub struct Point {
    pub x: i32,
    /// Hidden field.
    y: i32,
    pub(crate) z: i32,
}

/// Hidden.
#[derive(Debug)]
struct Secret;

pub(crate) fn crate_only() {}
pub(super) fn super_only() {}
pub(in crate::a) fn path_only() {}

pub fn api() -> i32 {
    let hidden = 1;
    hidden + 2
}

impl Point {
    pub fn x(&self) -> i32 {
        let x = self.x;
        x + 1
    }

    /// Hidden method.
    #[inline]
    fn private(&self) {}
}

impl Secret {
    fn only_private() {}
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, \"{}\", self.x)
    }
}

pub trait Area {
    fn area(&self) -> f64;
}

pub mod shapes {
    pub const SIDES: u8 = 4;
    const HIDDEN: u8 = 0;
    pub(crate) mod inner {}
}

mod private_mod {
    pub fn unreachable() {}
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {}
}
";

#[test]
fn rust_outline_keeps_only_pub_items_and_re_exports() {
    let out = compress(Some(Language::Rust), RUST_OUTLINE, ModeArg::Outline, true);

    assert_eq!(out.mode, ModeArg::Outline);
    assert_eq!(out.fallback, None);
    assert_reparses(Language::Rust, &out.text);
    insta::assert_snapshot!(out.text);
}

#[test]
fn rust_outline_with_no_docs_still_reparses() {
    let out = compress(Some(Language::Rust), RUST_OUTLINE, ModeArg::Outline, false);

    assert!(!out.text.contains("///"), "{}", out.text);
    assert_reparses(Language::Rust, &out.text);
}

#[test]
fn outline_for_the_other_languages_still_gets_skeleton_until_built() {
    let out = compress(Some(Language::Go), "package a\n", ModeArg::Outline, true);

    assert_eq!(out.mode, ModeArg::Skeleton);
    assert_eq!(out.fallback.map(|f| f.reason()), Some("mode_not_built"));
}

#[test]
fn the_file_tree_is_identical_across_modes() {
    let dir = fixture(&[("lib.rs", RUST_OUTLINE), ("src/b.rs", "pub fn b() {}\n")]);

    let trees: Vec<String> = ["-0", "-1", "-2"]
        .map(|flag| {
            let out = run(
                dir.path(),
                &[flag, "--report", "none", "lib.rs", "src/b.rs"],
            )
            .stdout;
            out[..out.find("<file ").unwrap()]
                .lines()
                .filter(|l| !l.starts_with("<bundle"))
                .collect()
        })
        .into();
    assert!(trees[0].contains("lib.rs"), "{}", trees[0]);
    assert_eq!(trees[0], trees[1]);
    assert_eq!(trees[1], trees[2]);
}

#[test]
fn rust_outline_drops_private_union_fields() {
    let source = "pub union U {\n    pub a: u32,\n    b: f32,\n}\n";
    let out = compress(Some(Language::Rust), source, ModeArg::Outline, true);

    assert_eq!(out.text, "pub union U {\n    pub a: u32,\n}\n");
}
