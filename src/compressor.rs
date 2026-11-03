use std::ops::Range;

use crate::cli::ModeArg;
use crate::parser::{self, Language};

/// D13: above this share of ERROR bytes, structural edits could mangle the file.
const MAX_ERROR_RATIO: f32 = 0.10;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fallback {
    ModeNotBuilt,
    ParseErrors { ratio: f32 },
    PlainText,
}

impl Fallback {
    pub fn reason(&self) -> &'static str {
        match self {
            Fallback::ModeNotBuilt => "mode_not_built",
            Fallback::ParseErrors { .. } => "parse_errors",
            Fallback::PlainText => "plain_text",
        }
    }

    pub fn note(&self) -> &'static str {
        match self {
            Fallback::ModeNotBuilt => "skeleton and outline are not built yet",
            Fallback::ParseErrors { .. } => "over 10% parse errors",
            Fallback::PlainText => "Plain Text: whitespace only",
        }
    }
}

pub struct Compressed {
    pub text: String,
    pub mode: ModeArg,
    pub fallback: Option<Fallback>,
}

pub fn compress(lang: Option<Language>, source: &str, mode: ModeArg, docs: bool) -> Compressed {
    let Some(tree) = lang.and_then(|lang| parser::parse(lang, source)) else {
        return Compressed {
            text: normalise(source, &[]),
            mode: ModeArg::Strip,
            fallback: Some(Fallback::PlainText),
        };
    };
    let mut edits = Vec::new();
    let mut literals = Vec::new();
    let mut error_bytes = 0;
    let mut stack = vec![(tree.root_node(), false)];
    while let Some((node, in_error)) = stack.pop() {
        if matches!(node.kind(), "line_comment" | "block_comment") {
            if !docs || node.child_by_field_name("doc").is_none() {
                edits.push(removal(source, node.byte_range()));
            }
            continue;
        }
        if matches!(node.kind(), "string_literal" | "raw_string_literal")
            && source[node.byte_range()].contains('\n')
        {
            literals.push(node.byte_range());
            continue;
        }
        if node.is_error() && !in_error {
            error_bytes += node.byte_range().len();
        }
        let mut cursor = node.walk();
        stack.extend(
            node.children(&mut cursor)
                .map(|child| (child, in_error || node.is_error())),
        );
    }
    let ratio = error_bytes as f32 / source.len().max(1) as f32;
    let fallback = if ratio > MAX_ERROR_RATIO {
        Some(Fallback::ParseErrors { ratio })
    } else if mode > ModeArg::Strip {
        // shortcut: skeleton and outline aren't built, so they get strip; drop once #40–#48 land.
        Some(Fallback::ModeNotBuilt)
    } else {
        None
    };
    Compressed {
        text: apply(source, edits, &literals),
        mode: ModeArg::Strip,
        fallback,
    }
}

/// A comment alone on its lines takes those lines with it; one between two tokens leaves a space
/// so they don't fuse.
fn removal(source: &str, comment: Range<usize>) -> (Range<usize>, &'static str) {
    let bytes = source.as_bytes();
    let start = comment.start;
    // Rust doc line comments own their newline.
    let end = comment.end - usize::from(bytes[comment.end - 1] == b'\n');
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let line_end = source[end..].find('\n').map_or(source.len(), |i| end + i);
    let blank = |s: &str| s.bytes().all(|b| b.is_ascii_whitespace());
    if blank(&source[line_start..start]) {
        let rest = &source[end..line_end];
        if blank(rest) {
            return (line_start..(line_end + 1).min(source.len()), "");
        }
        return (start..line_end - rest.trim_start().len(), "");
    }
    let solid = |b: Option<&u8>| b.is_some_and(|b| !b.is_ascii_whitespace());
    let glue = start > 0 && solid(bytes.get(start - 1)) && solid(bytes.get(end));
    (start..end, if glue { " " } else { "" })
}

/// `literals` are multi-line string literals, whose whitespace is data and survives normalising.
fn apply(source: &str, mut edits: Vec<(Range<usize>, &str)>, literals: &[Range<usize>]) -> String {
    edits.sort_by_key(|(range, _)| range.start);
    let mut at = 0;
    edits.retain(|(range, _)| {
        let disjoint = range.start >= at;
        at = at.max(range.end);
        disjoint
    });
    let shift = |pos: usize| -> usize {
        let removed: usize = edits
            .iter()
            .filter(|(range, _)| range.end <= pos)
            .map(|(range, replacement)| range.len() - replacement.len())
            .sum();
        pos - removed
    };
    let literals: Vec<_> = literals
        .iter()
        .map(|range| shift(range.start)..shift(range.end))
        .collect();
    let mut out = String::with_capacity(source.len());
    let mut at = 0;
    for (range, replacement) in &edits {
        out.push_str(&source[at..range.start]);
        out.push_str(replacement);
        at = range.end;
    }
    out.push_str(&source[at..]);
    normalise(&out, &literals)
}

/// Trims trailing whitespace, drops leading and trailing blank lines and collapses inner runs to one,
/// except on lines that end inside `keep`.
fn normalise(text: &str, keep: &[Range<usize>]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut after_blank = true;
    let mut offset = 0;
    for raw in text.split_inclusive('\n') {
        let newline = offset + raw.len() - 1;
        offset += raw.len();
        if keep.iter().any(|r| r.start < newline && newline < r.end) {
            out.push_str(raw);
            after_blank = false;
            continue;
        }
        let line = raw.trim_end();
        if line.is_empty() {
            if after_blank {
                continue;
            }
            after_blank = true;
        } else {
            after_blank = false;
        }
        out.push_str(line);
        out.push('\n');
    }
    if out.ends_with("\n\n") {
        out.pop();
    }
    out
}
