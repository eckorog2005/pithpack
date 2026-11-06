use std::ops::Range;

use tree_sitter::Node;

use crate::cli::ModeArg;
use crate::parser::{self, Language};

/// D13: above this share of ERROR bytes, structural edits could mangle the file.
const MAX_ERROR_RATIO: f32 = 0.10;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Fallback {
    StripOnly,
    ParseErrors { ratio: f32 },
    PlainText,
}

impl Fallback {
    pub fn reason(&self) -> &'static str {
        match self {
            Fallback::StripOnly => "strip_only",
            Fallback::ParseErrors { .. } => "parse_errors",
            Fallback::PlainText => "plain_text",
        }
    }

    pub fn note(&self) -> &'static str {
        match self {
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
    // Without errors no fallback can restore what is inside an elided body, so it needn't be walked.
    let can_skip_bodies = mode != ModeArg::Strip && !tree.root_node().has_error();
    let mut cursor = tree.walk();
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
        let body_node = matches!(
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
        .then(|| node.child_by_field_name("body"))
        .flatten()
        // `x => ({ … })` is an expression; swapping it for a block would change what it returns.
        .filter(|body| node.kind() != "arrow_function" || body.kind() == "statement_block");
        let body_range = body_node.and_then(|body| {
            // A kept Python docstring stays as the first line of the collapsed body.
            let start = match docstring(source, body).filter(|_| docs) {
                Some((doc, _)) => doc.next_named_sibling()?,
                None => body,
            };
            (start.start_position().row != body.end_position().row)
                .then(|| start.start_byte()..body.end_byte())
        });
        let skipped_body = body_node.filter(|body| {
            can_skip_bodies
                && body_range
                    .as_ref()
                    .is_some_and(|r| r.start == body.start_byte())
        });
        // A nested fn's body overlaps its parent's, and `apply` keeps only the outer edit.
        bodies.extend(body_range);
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
        stack.extend(
            node.children(&mut cursor)
                .filter(|child| {
                    Some(*child) != dropped.map(|(doc, _)| doc) && Some(*child) != skipped_body
                })
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
        let private_items = match lang {
            Some(Language::Rust) => Some(rust_private_items as fn(Node, &str) -> Vec<Range<usize>>),
            Some(Language::Python) => Some(python_private_items as _),
            Some(Language::Go) => Some(go_private_items as _),
            Some(Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx) => {
                Some(ts_private_items as _)
            }
            _ => None,
        }
        .filter(|_| mode == ModeArg::Outline);
        if let Some(private_items) = private_items {
            for item in private_items(tree.root_node(), source) {
                let (range, glue) = removal(source, item.clone());
                edits.retain(|(edit, _)| !(edit.start < range.end && range.start < edit.end));
                edits.push((range, glue));
                literals
                    .retain(|literal| !(item.start <= literal.start && literal.end <= item.end));
            }
            (ModeArg::Outline, None)
        } else {
            (ModeArg::Skeleton, None)
        }
    } else {
        (ModeArg::Strip, Some(Fallback::StripOnly))
    };
    Compressed {
        text: apply(source, edits, &literals),
        mode,
        fallback,
    }
}

fn is_pub(item: Node, source: &str) -> bool {
    let mut cursor = item.walk();
    let vis = item
        .children(&mut cursor)
        .find(|child| child.kind() == "visibility_modifier");
    vis.is_some_and(|vis| &source[vis.byte_range()] == "pub")
}

/// Trait bodies and trait impls keep their members: their visibility is inherited.
fn rust_private_items(root: Node, source: &str) -> Vec<Range<usize>> {
    let mut dropped = Vec::new();
    let mut scopes = vec![root];
    while let Some(scope) = scopes.pop() {
        let mut lead = None;
        let mut cursor = scope.walk();
        for child in scope.named_children(&mut cursor) {
            let text = &source[child.byte_range()];
            match child.kind() {
                "line_comment" | "block_comment"
                    if !text.starts_with("//!") && !text.starts_with("/*!") =>
                {
                    lead.get_or_insert(child.start_byte());
                    continue;
                }
                "attribute_item" => {
                    lead.get_or_insert(child.start_byte());
                    continue;
                }
                "function_item"
                | "struct_item"
                | "enum_item"
                | "union_item"
                | "trait_item"
                | "type_item"
                | "const_item"
                | "static_item"
                | "mod_item"
                | "use_declaration"
                | "field_declaration"
                | "extern_crate_declaration" => {
                    let keep = is_pub(child, source);
                    if keep {
                        let body = match child.kind() {
                            "mod_item" | "struct_item" | "union_item" => {
                                child.child_by_field_name("body")
                            }
                            _ => None,
                        };
                        scopes.extend(body);
                    } else {
                        let comma = child.next_sibling().filter(|next| next.kind() == ",");
                        let end = comma.map_or(child.end_byte(), |comma| comma.end_byte());
                        dropped.push(lead.unwrap_or(child.start_byte())..end);
                    }
                }
                "impl_item" if child.child_by_field_name("trait").is_none() => {
                    let body = child.child_by_field_name("body");
                    let mut cursor = child.walk();
                    let has_pub = body.is_some_and(|body| {
                        body.named_children(&mut cursor)
                            .any(|member| is_pub(member, source))
                    });
                    if has_pub {
                        scopes.extend(body);
                    } else {
                        dropped.push(lead.unwrap_or(child.start_byte())..child.end_byte());
                    }
                }
                _ => {}
            }
            lead = None;
        }
    }
    dropped
}

fn with_doc(node: Node) -> Range<usize> {
    let mut start = node;
    while let Some(prev) = start
        .prev_sibling()
        .filter(|prev| prev.kind() == "comment")
        .filter(|prev| prev.end_position().row + 1 == start.start_position().row)
    {
        start = prev;
    }
    start.start_byte()..node.end_byte()
}

fn go_private_items(root: Node, source: &str) -> Vec<Range<usize>> {
    let exported = |name: &str| name.chars().next().is_some_and(char::is_uppercase);
    let named_exported = |node: Node| {
        let mut cursor = node.walk();
        let mut names = node.children_by_field_name("name", &mut cursor);
        names.any(|name| exported(&source[name.byte_range()]))
    };
    let embedded_exported = |field: Node| {
        let ty = field
            .child_by_field_name("type")
            .map_or("", |ty| &source[ty.byte_range()]);
        exported(
            ty.trim_start_matches('*')
                .rsplit('.')
                .next()
                .unwrap_or_default(),
        )
    };
    let mut dropped = Vec::new();
    let mut cursor = root.walk();
    for decl in root.named_children(&mut cursor) {
        match decl.kind() {
            "import_declaration" => dropped.push(with_doc(decl)),
            "function_declaration" | "method_declaration" => {
                if !named_exported(decl) {
                    dropped.push(with_doc(decl));
                }
            }
            "type_declaration" | "const_declaration" | "var_declaration" => {
                let mut cursor = decl.walk();
                let specs: Vec<Node> = decl
                    .named_children(&mut cursor)
                    .filter(|spec| spec.kind().ends_with("_spec") || spec.kind() == "type_alias")
                    .collect();
                let (kept, gone): (Vec<Node>, Vec<Node>) =
                    specs.into_iter().partition(|spec| named_exported(*spec));
                if kept.is_empty() {
                    dropped.push(with_doc(decl));
                    continue;
                }
                dropped.extend(gone.into_iter().map(with_doc));
                for spec in kept.iter().filter(|spec| spec.kind() == "type_spec") {
                    let Some(ty) = spec.child_by_field_name("type") else {
                        continue;
                    };
                    let mut cursor = ty.walk();
                    let mut members: Vec<Node> = ty.named_children(&mut cursor).collect();
                    if ty.kind() == "struct_type" {
                        members = members
                            .into_iter()
                            .flat_map(|list| {
                                let mut cursor = list.walk();
                                list.named_children(&mut cursor).collect::<Vec<_>>()
                            })
                            .collect();
                    }
                    for member in members {
                        let private = match member.kind() {
                            "field_declaration" if member.child_by_field_name("name").is_some() => {
                                !named_exported(member)
                            }
                            "field_declaration" => !embedded_exported(member),
                            "method_elem" => !named_exported(member),
                            _ => false,
                        };
                        if private {
                            dropped.push(with_doc(member));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    dropped
}

fn ts_private_items(root: Node, source: &str) -> Vec<Range<usize>> {
    let is_private = |member: Node| {
        let mut cursor = member.walk();
        let private = member.children(&mut cursor).any(|child| {
            child.kind() == "accessibility_modifier" && &source[child.byte_range()] == "private"
        });
        private
            || ["name", "property"].iter().any(|field| {
                member
                    .child_by_field_name(field)
                    .is_some_and(|name| name.kind() == "private_property_identifier")
            })
    };
    let mut dropped = Vec::new();
    let mut cursor = root.walk();
    for stmt in root.named_children(&mut cursor) {
        match stmt.kind() {
            "comment" | "hash_bang_line" => {}
            "export_statement" => {
                let Some(class) = stmt
                    .child_by_field_name("declaration")
                    .filter(|decl| {
                        matches!(
                            decl.kind(),
                            "class_declaration" | "abstract_class_declaration"
                        )
                    })
                    .or_else(|| {
                        stmt.child_by_field_name("value")
                            .filter(|v| v.kind() == "class")
                    })
                else {
                    continue;
                };
                let Some(body) = class.child_by_field_name("body") else {
                    continue;
                };
                let mut cursor = body.walk();
                dropped.extend(
                    body.named_children(&mut cursor)
                        .filter(|member| member.kind() != "comment" && is_private(*member))
                        .map(|member| {
                            let range = with_doc(member);
                            // Fields end in a separate `;` token that would be left behind.
                            match member.next_sibling().filter(|next| next.kind() == ";") {
                                Some(semi) => range.start..semi.end_byte(),
                                None => range,
                            }
                        }),
                );
            }
            _ => dropped.push(with_doc(stmt)),
        }
    }
    dropped
}

fn python_private_items(root: Node, source: &str) -> Vec<Range<usize>> {
    let mut cursor = root.walk();
    let statements: Vec<Node> = root.named_children(&mut cursor).collect();
    fn assignment(stmt: Node) -> Option<Node> {
        let node = stmt
            .named_child(0)
            .filter(|_| stmt.kind() == "expression_statement")?;
        matches!(node.kind(), "assignment" | "augmented_assignment").then_some(node)
    }
    let is_all_assignment = |node: Node| {
        node.child_by_field_name("left")
            .is_some_and(|left| &source[left.byte_range()] == "__all__")
    };
    let mut exported: Option<Vec<&str>> = None;
    for stmt in &statements {
        let Some(node) = assignment(*stmt).filter(|node| is_all_assignment(*node)) else {
            continue;
        };
        let names = exported.get_or_insert_default();
        let Some(list) = node.child_by_field_name("right") else {
            continue;
        };
        let mut cursor = list.walk();
        for string in list
            .named_children(&mut cursor)
            .filter(|n| n.kind() == "string")
        {
            let mut cursor = string.walk();
            names.extend(
                string
                    .named_children(&mut cursor)
                    .filter(|part| part.kind() == "string_content")
                    .map(|part| &source[part.byte_range()]),
            );
        }
    }
    let public = |name: &str| match &exported {
        Some(all) => name == "__all__" || all.contains(&name),
        None => !name.starts_with('_'),
    };
    let bound_names = |import: Node| {
        let mut cursor = import.walk();
        import
            .children_by_field_name("name", &mut cursor)
            .map(|name| {
                let name = name.child_by_field_name("alias").unwrap_or(name);
                source[name.byte_range()]
                    .split('.')
                    .next()
                    .unwrap_or_default()
            })
            .collect::<Vec<&str>>()
    };
    statements
        .into_iter()
        .filter(|stmt| match stmt.kind() {
            "function_definition" | "class_definition" | "decorated_definition" => stmt
                .child_by_field_name("definition")
                .unwrap_or(*stmt)
                .child_by_field_name("name")
                .is_some_and(|name| !public(&source[name.byte_range()])),
            "import_statement" | "import_from_statement" => {
                exported.is_none() || !bound_names(*stmt).iter().any(|name| public(name))
            }
            _ => assignment(*stmt)
                .and_then(|node| node.child_by_field_name("left"))
                .filter(|left| left.kind() == "identifier")
                .is_some_and(|left| !public(&source[left.byte_range()])),
        })
        .map(|stmt| stmt.byte_range())
        .collect()
}

// shortcut: Python node kinds in the shared walk; move them to tree-sitter queries once more
// grammars need them.
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
