use std::sync::OnceLock;

use rayon::prelude::*;
use tiktoken_rs::CoreBPE;

const CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
pub enum TokenizerKind {
    O200k,
    Cl100k,
}

pub struct Encoder {
    bpe: CoreBPE,
    name: &'static str,
}

static O200K: OnceLock<Encoder> = OnceLock::new();
static CL100K: OnceLock<Encoder> = OnceLock::new();

impl Encoder {
    /// The first call per kind builds the BPE, the largest fixed startup cost.
    pub fn get(kind: TokenizerKind) -> &'static Encoder {
        match kind {
            TokenizerKind::O200k => O200K.get_or_init(|| Encoder {
                bpe: tiktoken_rs::o200k_base().expect("o200k_base ranks are embedded"),
                name: "o200k_base",
            }),
            TokenizerKind::Cl100k => CL100K.get_or_init(|| Encoder {
                bpe: tiktoken_rs::cl100k_base().expect("cl100k_base ranks are embedded"),
                name: "cl100k_base",
            }),
        }
    }

    /// Special tokens such as `<|endoftext|>` are counted as plain text.
    pub fn count(&self, text: &str) -> usize {
        chunks(text)
            .par_iter()
            .map(|chunk| self.bpe.encode_ordinary(chunk).len())
            .sum()
    }

    /// `count(whole)`, without re-tokenizing the `known` substrings (text, its count) it holds in
    /// order; one that is missing or not cut off from its surroundings just gets counted again.
    pub fn count_with_known<'a>(
        &self,
        whole: &str,
        known: impl IntoIterator<Item = (&'a str, usize)>,
    ) -> usize {
        let mut total = 0;
        let mut at = 0;
        for (text, tokens) in known {
            let Some(start) = whole[at..].find(text).map(|i| at + i) else {
                continue;
            };
            let end = start + text.len();
            if !text.is_empty() && is_cut(whole, start) && is_cut(whole, end) {
                total += tokens + self.count(&whole[at..start]);
                at = end;
            }
        }
        total + self.count(&whole[at..])
    }

    pub fn name(&self) -> &'static str {
        self.name
    }
}

/// A tokenizer piece never spans `pos`, so the counts on either side add up to the whole's.
fn is_cut(text: &str, pos: usize) -> bool {
    let bytes = text.as_bytes();
    // Only whitespace, or `/` under o200k's `[\r\n/]*`, can continue a piece past a newline.
    pos > 0
        && bytes[pos - 1] == b'\n'
        && bytes
            .get(pos)
            .is_some_and(|&b| b.is_ascii_graphic() && b != b'/')
}

fn chunks(mut text: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    while text.len() > CHUNK_BYTES {
        let Some(cut) = (CHUNK_BYTES..text.len()).find(|&pos| is_cut(text, pos)) else {
            break;
        };
        let (chunk, rest) = text.split_at(cut);
        chunks.push(chunk);
        text = rest;
    }
    chunks.push(text);
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_count_matches_the_whole_text_for_both_tokenizers() {
        let awkward =
            "fn a() {}\n// c\n}\n/* d */\n<file>\n\n  x\r\n\ty's\n's\n1234\n\u{a0}é\n \n🦀\n/\n";
        let text = awkward.repeat(3 * CHUNK_BYTES / awkward.len());
        assert!(chunks(&text).len() > 1);
        for kind in [TokenizerKind::O200k, TokenizerKind::Cl100k] {
            let encoder = Encoder::get(kind);
            assert_eq!(
                encoder.count(&text),
                encoder.bpe.encode_ordinary(&text).len()
            );
        }
    }

    #[test]
    fn known_counts_add_up_to_the_whole_whatever_surrounds_them() {
        for (whole, texts) in [
            ("<a>\nfn x() {}\n</a>\n", vec!["fn x() {}\n"]),
            // `>\n//` and `}\n//` are single pieces, so these must not be counted apart.
            (
                "<a>\n// doc\nfn x() {}\n</a>\n",
                vec!["// doc\nfn x() {}\n"],
            ),
            ("<a>\nfn x() {}\n// tail\n", vec!["fn x() {}\n"]),
            ("<a>\n  indented\n</a>\n", vec!["  indented\n"]),
            ("<a>\nno newline</a>\n", vec!["no newline"]),
            ("<a>\nx\n</a>\n<a>\ny\n</a>\n", vec!["x\n", "y\n"]),
            (
                "<a>\nescaped\n</a>\n",
                vec!["not in the whole\n", "escaped\n", ""],
            ),
        ] {
            for kind in [TokenizerKind::O200k, TokenizerKind::Cl100k] {
                let encoder = Encoder::get(kind);
                let known = texts.iter().map(|text| (*text, encoder.count(text)));
                let found = encoder.count_with_known(whole, known);
                assert_eq!(found, encoder.bpe.encode_ordinary(whole).len(), "{whole:?}");
            }
        }
    }
}
