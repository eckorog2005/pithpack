mod language;

use std::cell::RefCell;

pub use language::{Language, detect, lang_name};

/// `None` if tree-sitter gives up; the file is then treated as Plain Text.
pub fn parse(lang: Language, source: &str) -> Option<tree_sitter::Tree> {
    let grammar: tree_sitter::Language = match lang {
        Language::Rust => tree_sitter_rust::LANGUAGE.into(),
        Language::Python => tree_sitter_python::LANGUAGE.into(),
        Language::Go => tree_sitter_go::LANGUAGE.into(),
        Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Language::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Language::JavaScript | Language::Jsx => tree_sitter_javascript::LANGUAGE.into(),
        Language::Html => tree_sitter_html::LANGUAGE.into(),
        Language::Css => tree_sitter_css::LANGUAGE.into(),
    };
    thread_local! {
        static PARSER: RefCell<tree_sitter::Parser> = RefCell::new(tree_sitter::Parser::new());
    }
    PARSER.with_borrow_mut(|parser| {
        parser
            .set_language(&grammar)
            .expect("the bundled grammar matches the tree-sitter ABI");
        parser.parse(source, None)
    })
}
