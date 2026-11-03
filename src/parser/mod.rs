mod language;

pub use language::{Language, detect, lang_name};

/// `None` until the language's grammar is wired in; such files are treated as Plain Text.
pub fn parse(lang: Language, source: &str) -> Option<tree_sitter::Tree> {
    let grammar: tree_sitter::Language = match lang {
        Language::Rust => tree_sitter_rust::LANGUAGE.into(),
        _ => return None,
    };
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&grammar)
        .expect("the bundled grammar matches the tree-sitter ABI");
    parser.parse(source, None)
}
