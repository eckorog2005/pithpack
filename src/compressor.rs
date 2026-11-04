use std::ops::Range;

use tree_sitter::Node;

use crate::cli::ModeArg;
use crate::parser::{self, Language};

/// D13: above this share of ERROR bytes, structural edits could mangle the file.
const MAX_ERROR_RATIO: f32 = 0.10;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fallback {
    ModeNotBuilt,
    StripOnly,
    ParseErrors { ratio: f32 },
    PlainText,
}

impl Fallback {
    pub fn reason(&self) -> &'static str {
        match self {
            Fallback::ModeNotBuilt => "mode_not_built",
            Fallback::StripOnly => "strip_only",
            Fallback::ParseErrors { .. } => "parse_errors",
            Fallback::PlainText => "plain_text",
        }
    }

    pub fn note(&self) -> &'static str {
        match self {
            Fallback::ModeNotBuilt => "Mode not built for this language yet",
            Fallback::StripOnly => "strip is the only Mode for this language",
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
    let mut bodies = Vec::new();
    let mut error_bytes = 0;
    let mut stack = vec![(tree.root_node(), false)];
    while let Some((node, in_error)) = stack.pop() {
        if matches!(node.kind(), "line_comment" | "block_comment" | "comment") {
            // tree-sitter-html lexes `<!--` inside tags and RCDATA elements too, where it is text.
            if std::iter::successors(node.parent(), Node::parent).any(|ancestor| {
                matches!(ancestor.kind(), "start_tag" | "self_closing_tag")
                    || (ancestor.kind() == "element"
                        && ancestor
                            .child(0)
                            .and_then(|tag| tag.named_child(0))
                            .is_some_and(|name| {
                                let name = &source[name.byte_range()];
                                name.eq_ignore_ascii_case("textarea")
                                    || name.eq_ignore_ascii_case("title")
                            }))
            }) {
                continue;
            }
            let doc = node.child_by_field_name("doc").is_some()
                || match lang {
                    Some(Language::Go) => is_go_doc(source, node),
                    Some(
                        Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx,
                    ) => is_jsdoc(source, node),
                    _ => false,
                };
            if !docs || !doc {
                // `{/* … */}` emptied of comments would leave a stray `{ }` in the markup.
                let target = node
                    .parent()
                    .filter(|parent| {
                        let mut cursor = parent.walk();
                        parent.kind() == "jsx_expression"
                            && parent
                                .named_children(&mut cursor)
                                .all(|child| child.kind() == "comment")
                    })
                    .unwrap_or(node);
                let (range, glue) = removal(source, target.byte_range());
                // An HTML comment never sits inside a tag, so a space would only change the text.
                edits.push((
                    range,
                    if lang == Some(Language::Html) {
                        ""
                    } else {
                        glue
                    },
                ));
            }
            continue;
        }
        if matches!(
            node.kind(),
            "string_literal" | "raw_string_literal" | "string" | "template_string"
        ) && source[node.byte_range()].contains('\n')
        {
            literals.push(node.byte_range());
            continue;
        }
        let body = node
            .child_by_field_name("body")
            .filter(|_| {
                matches!(
                    node.kind(),
                    "function_item"
                        | "function_definition"
                        | "function_declaration"
                        | "method_declaration"
                        | "generator_function_declaration"
                        | "function_expression"
                        | "generator_function"
                        | "arrow_function"
                        | "method_definition"
                )
            })
            // `x => ({ … })` is an expression; swapping it for a block would change what it returns.
            .filter(|body| node.kind() != "arrow_function" || body.kind() == "statement_block")
            .and_then(|body| {
                // A kept Python docstring stays as the first line of the collapsed body.
                let start = match docstring(source, body).filter(|_| docs) {
                    Some((doc, _)) => doc.next_named_sibling()?,
                    None => body,
                };
                (start.start_position().row != body.end_position().row)
                    .then(|| start.start_byte()..body.end_byte())
            });
        // A nested fn's body overlaps its parent's, and `apply` keeps only the outer edit.
        bodies.extend(body);
        if node.is_error() && !in_error {
            error_bytes += node.byte_range().len();
        }
        let dropped = if docs { None } else { docstring(source, node) };
        if let Some((doc, sole)) = dropped {
            let mut range = doc.byte_range();
            if let Some(semicolon) = doc.next_sibling().filter(|next| next.kind() == ";") {
                range.end = semicolon.end_byte();
            }
            // An emptied block is a syntax error, so a body that was only its docstring gets `...`.
            edits.push(if sole {
                (range, "...")
            } else {
                removal(source, range)
            });
        }
        let mut cursor = node.walk();
        stack.extend(
            node.children(&mut cursor)
                .filter(|child| Some(*child) != dropped.map(|(doc, _)| doc))
                .map(|child| (child, in_error || node.is_error())),
        );
    }
    let ratio = error_bytes as f32 / source.len().max(1) as f32;
    let (mode, fallback) = if ratio > MAX_ERROR_RATIO {
        (ModeArg::Strip, Some(Fallback::ParseErrors { ratio }))
    } else if mode == ModeArg::Strip {
        (ModeArg::Strip, None)
    } else if let Some(placeholder) = match lang {
        // `{ … }` alone is an ERROR node; the comment keeps the placeholder parseable.
        Some(Language::Rust | Language::Go) => Some("{ /* … */ }"),
        Some(Language::Python) => Some("..."),
        Some(Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx) => {
            Some("{ … }")
        }
        _ => None,
    } {
        // A dropped Python docstring starts its line, before its body, so it would win in `apply`.
        edits.retain(|(range, _)| {
            !bodies
                .iter()
                .any(|body| range.start < body.end && body.start < range.end)
        });
        edits.extend(bodies.iter().map(|body| (body.clone(), placeholder)));
        literals.retain(|literal| {
            !bodies
                .iter()
                .any(|body| body.start <= literal.start && literal.end <= body.end)
        });
        // shortcut: outline isn't built, so it gets skeleton; drop once #45–#48 land.
        let fallback = (mode == ModeArg::Outline).then_some(Fallback::ModeNotBuilt);
        (ModeArg::Skeleton, fallback)
    } else {
        (ModeArg::Strip, Some(Fallback::StripOnly))
    };
    Compressed {
        text: apply(source, edits, &literals),
        mode,
        fallback,
    }
}

// shortcut: Python node kinds in the shared walk; move them to `@comment.doc` queries
// (SDD §8.2) once more grammars need them.
/// A Python docstring: the first statement of a module, class or function body when it is a bare
/// string. `true` when it is the only statement of a class or function body.
fn docstring<'a>(source: &str, body: Node<'a>) -> Option<(Node<'a>, bool)> {
    let owns_docstring = match body.kind() {
        "module" => true,
        "block" => matches!(
            body.parent()?.kind(),
            "class_definition" | "function_definition"
        ),
        _ => false,
    };
    if !owns_docstring {
        return None;
    }
    let mut cursor = body.walk();
    let mut statements = body
        .named_children(&mut cursor)
        .filter(|child| child.kind() != "comment");
    let first = statements.next()?;
    let only_child = (first.kind() == "expression_statement" && first.named_child_count() == 1)
        .then(|| first.named_child(0))??;
    // f-strings and bytes are never docstrings, and an f-string's `{…}` may have side effects.
    let prefix = &source[only_child.child(0)?.byte_range()];
    (only_child.kind() == "string" && !prefix.contains(['f', 'F', 'b', 'B']))
        .then_some((first, body.kind() == "block" && statements.next().is_none()))
}

/// A Go Doc Comment: part of a block of own-line comments whose last line sits directly above a
/// top-level declaration (or a spec in a grouped one), struct field or interface method.
fn is_go_doc(source: &str, comment: Node) -> bool {
    let start = comment.start_byte();
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    if !source[line_start..start].trim().is_empty() {
        return false;
    }
    let mut node = comment;
    while let Some(next) = node.next_sibling() {
        if next.start_position().row != node.end_position().row + 1 {
            return false;
        }
        if next.kind() != "comment" {
            let member = matches!(next.kind(), "field_declaration" | "method_elem");
            let declaration = matches!(
                next.kind(),
                "package_clause"
                    | "function_declaration"
                    | "method_declaration"
                    | "type_declaration"
                    | "const_declaration"
                    | "var_declaration"
                    | "type_spec"
                    | "const_spec"
                    | "var_spec"
            );
            let in_function = std::iter::successors(next.parent(), Node::parent)
                .any(|ancestor| ancestor.kind() == "block");
            return member || (declaration && !in_function);
        }
        node = next;
    }
    false
}

/// A JSDoc: a `/** … */` comment directly above a declaration, member or assignment, possibly
/// through other comments.
fn is_jsdoc(source: &str, comment: Node) -> bool {
    let text = &source[comment.byte_range()];
    if !text.starts_with("/**") || text == "/**/" {
        return false;
    }
    let mut node = comment;
    while let Some(next) = node.next_sibling() {
        if next.start_position().row > node.end_position().row + 1 {
            return false;
        }
        if next.kind() != "comment" {
            // `module.exports.x = …` and `this.x = …` are how plain JS declares API.
            let assignment = next.kind() == "expression_statement"
                && next
                    .named_child(0)
                    .is_some_and(|expression| expression.kind() == "assignment_expression");
            return assignment
                || matches!(
                    next.kind(),
                    "export_statement"
                        | "function_declaration"
                        | "generator_function_declaration"
                        | "class_declaration"
                        | "abstract_class_declaration"
                        | "lexical_declaration"
                        | "variable_declaration"
                        | "method_definition"
                        | "field_definition"
                        | "public_field_definition"
                        | "pair"
                        | "interface_declaration"
                        | "type_alias_declaration"
                        | "enum_declaration"
                        | "enum_assignment"
                        | "property_identifier"
                        | "module"
                        | "internal_module"
                        | "ambient_declaration"
                        | "function_signature"
                        | "method_signature"
                        | "abstract_method_signature"
                        | "property_signature"
                );
        }
        node = next;
    }
    false
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
