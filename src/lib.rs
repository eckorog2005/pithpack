pub mod cli;
pub mod compressor;
pub mod config;
pub mod encoder;
pub mod formatter;
pub mod parser;
pub mod redact;
pub mod report;
pub mod scanner;
pub mod sink;

use std::borrow::Cow;
use std::io::{Read, Write};
use std::time::Instant;

use anyhow::Context;
use rayon::prelude::*;

use config::Options;
use encoder::Encoder;
use formatter::{BundleMeta, FileBlock};
use report::{FileStat, Report, ReportFormat};
use scanner::{Origin, Skip};

pub struct Io<'a> {
    pub stdin: &'a mut dyn Read,
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
    pub stderr_is_tty: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    OverBudget,
}

struct Processed {
    read_bytes: u64,
    redactions: usize,
    lang: Option<parser::Language>,
    text: String,
    compressed: compressor::Compressed,
}

fn group_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn run(opts: Options, io: Io) -> anyhow::Result<Outcome> {
    let started = Instant::now();
    let tokenizer = opts.tokenizer;
    let warm = std::thread::spawn(move || Encoder::get(tokenizer));
    if !opts.redact {
        writeln!(
            io.stderr,
            "warning: Redaction is disabled by --no-redact; secrets in Source Files reach the Bundle"
        )
        .context("cannot write a warning to stderr")?;
    }
    let rules = redact::Rules::with_extra(&opts.redaction_extra)?;
    let scan = scanner::scan(&opts)?;
    for (display, skip) in &scan.skipped {
        if let Skip::Unreadable(kind) = skip {
            writeln!(io.stderr, "warning: skipped unreadable `{display}`: {kind}")
                .context("cannot write a warning to stderr")?;
        }
    }
    let mut stdin_bytes = None;
    if scan.candidates.iter().any(|c| c.origin == Origin::Stdin) {
        let mut bytes = Vec::new();
        io.stdin
            .read_to_end(&mut bytes)
            .context("cannot read stdin")?;
        stdin_bytes = Some(bytes);
    }
    let processed: Vec<anyhow::Result<Processed>> = scan
        .candidates
        .par_iter()
        .map(|candidate| {
            let bytes = match (&candidate.origin, &stdin_bytes) {
                (Origin::Stdin, Some(bytes)) => Cow::Borrowed(bytes.as_slice()),
                _ => Cow::Owned(
                    std::fs::read(&candidate.path)
                        .with_context(|| format!("cannot read `{}`", candidate.display))?,
                ),
            };
            let raw = String::from_utf8_lossy(&bytes);
            let (text, redactions) = if opts.redact {
                let redacted = redact::redact(&raw, &rules);
                (redacted.text.into_owned(), redacted.hits.len())
            } else {
                (raw.into_owned(), 0)
            };
            let lang = match candidate.origin {
                Origin::Stdin => opts.stdin_lang,
                Origin::Explicit | Origin::Walked => parser::detect(&candidate.display),
            };
            let compressed = compressor::compress(lang, &text, opts.mode, opts.docs);
            Ok(Processed {
                read_bytes: bytes.len() as u64,
                redactions,
                lang,
                text,
                compressed,
            })
        })
        .collect();
    let processed = processed.into_iter().collect::<anyhow::Result<Vec<_>>>()?;
    let encoder = warm.join().expect("encoder initialisation panicked");
    let tokens: Vec<usize> = processed
        .par_iter()
        .map(|p| encoder.count(&p.compressed.text))
        .collect();
    let blocks: Vec<FileBlock> = scan
        .candidates
        .iter()
        .zip(&processed)
        .zip(&tokens)
        .map(|((c, p), &tokens)| FileBlock {
            display: &c.display,
            lang: p.lang,
            mode: p.compressed.mode.name(),
            tokens,
            text: &p.compressed.text,
        })
        .collect();
    let mut meta = BundleMeta {
        format: opts.format,
        // shortcut: labelled skeleton for outline, and files without skeleton are still strip (each
        // `<file>` says which); label by request once #44–#48 land.
        mode: opts.mode.min(cli::ModeArg::Skeleton).name(),
        tokenizer: encoder.name(),
        tokens: 0,
        tree: opts.tree,
    };
    // The total sits inside the text it counts; the re-render's digits may shift it by ±2.
    meta.tokens = encoder.count(&formatter::render(&meta, &blocks));
    let bundle = formatter::render(&meta, &blocks);
    sink::write(&opts.sink, &bundle, io.stdout)?;

    if opts.report != ReportFormat::None {
        let report = Report {
            files: blocks
                .par_iter()
                .zip(&processed)
                .map(|(b, p)| FileStat {
                    display: b.display.to_string(),
                    lang: b.lang,
                    requested_mode: opts.mode.name(),
                    mode: b.mode,
                    fallback: p.compressed.fallback,
                    original_bytes: p.read_bytes,
                    original_tokens: encoder.count(&p.text),
                    compressed_bytes: b.text.len() as u64,
                    compressed_tokens: b.tokens,
                    redactions: p.redactions,
                })
                .collect(),
            skipped: scan.skipped,
            bundle_tokens: meta.tokens,
            tokenizer: encoder.name(),
            elapsed: started.elapsed(),
            budget: opts.budget,
        };
        let rendered = match opts.report {
            ReportFormat::Json => report::render_json(&report),
            _ => report::render_table(&report, io.stderr_is_tty),
        };
        io.stderr
            .write_all(rendered.as_bytes())
            .context("cannot write the Report to stderr")?;
    }
    match opts.budget {
        Some(budget) if meta.tokens > budget => {
            if opts.report != ReportFormat::Json {
                writeln!(
                    io.stderr,
                    "warning: bundle is {} tokens, budget {}",
                    group_thousands(meta.tokens),
                    group_thousands(budget)
                )
                .context("cannot write a warning to stderr")?;
            }
            Ok(Outcome::OverBudget)
        }
        _ => Ok(Outcome::Ok),
    }
}
