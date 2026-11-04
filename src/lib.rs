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

use std::io::{Read, Write};
use std::time::Instant;

use anyhow::Context;

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

fn group_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
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
    let mut texts = Vec::with_capacity(scan.candidates.len());
    let mut read_bytes = Vec::with_capacity(scan.candidates.len());
    let mut redactions = Vec::with_capacity(scan.candidates.len());
    for candidate in &scan.candidates {
        let bytes = if candidate.origin == Origin::Stdin {
            let mut bytes = Vec::new();
            io.stdin
                .read_to_end(&mut bytes)
                .context("cannot read stdin")?;
            bytes
        } else {
            std::fs::read(&candidate.path)
                .with_context(|| format!("cannot read `{}`", candidate.display))?
        };
        read_bytes.push(bytes.len() as u64);
        let raw = String::from_utf8_lossy(&bytes);
        if opts.redact {
            let redacted = redact::redact(&raw, &rules);
            redactions.push(redacted.hits.len());
            texts.push(redacted.text.into_owned());
        } else {
            redactions.push(0);
            texts.push(raw.into_owned());
        }
    }
    let langs: Vec<_> = scan
        .candidates
        .iter()
        .map(|c| match c.origin {
            Origin::Stdin => opts.stdin_lang,
            Origin::Explicit | Origin::Walked => parser::detect(&c.display),
        })
        .collect();
    let compressed: Vec<_> = texts
        .iter()
        .zip(&langs)
        .map(|(text, &lang)| compressor::compress(lang, text, opts.mode, opts.docs))
        .collect();
    let encoder = warm.join().expect("encoder initialisation panicked");
    let blocks: Vec<FileBlock> = scan
        .candidates
        .iter()
        .zip(&langs)
        .zip(&compressed)
        .map(|((c, &lang), out)| FileBlock {
            display: &c.display,
            lang,
            mode: out.mode.name(),
            tokens: encoder.count(&out.text),
            text: &out.text,
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
                .iter()
                .zip(&compressed)
                .zip(&texts)
                .zip(&read_bytes)
                .zip(&redactions)
                .map(
                    |((((b, out), original), &original_bytes), &redactions)| FileStat {
                        display: b.display.to_string(),
                        lang: b.lang,
                        requested_mode: opts.mode.name(),
                        mode: b.mode,
                        fallback: out.fallback,
                        original_bytes,
                        original_tokens: encoder.count(original),
                        compressed_bytes: b.text.len() as u64,
                        compressed_tokens: b.tokens,
                        redactions,
                    },
                )
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
