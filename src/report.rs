use std::time::Duration;

use crate::compressor::Fallback;
use crate::parser::{self, Language};
use crate::scanner::Skip;

#[derive(Clone, Copy, PartialEq, Eq, Debug, clap::ValueEnum)]
pub enum ReportFormat {
    Table,
    Json,
    None,
}

pub struct FileStat {
    pub display: String,
    pub lang: Option<Language>,
    pub requested_mode: &'static str,
    pub mode: &'static str,
    pub fallback: Option<Fallback>,
    pub original_bytes: u64,
    pub original_tokens: usize,
    pub compressed_bytes: u64,
    pub compressed_tokens: usize,
    pub redactions: usize,
}

pub struct Report {
    pub files: Vec<FileStat>,
    pub skipped: Vec<(String, Skip)>,
    pub bundle_tokens: usize,
    pub tokenizer: &'static str,
    pub elapsed: Duration,
    pub budget: Option<usize>,
}

pub fn render_json(r: &Report) -> String {
    use serde_json::json;
    let files: Vec<_> = r
        .files
        .iter()
        .map(|f| {
            json!({
                "path": f.display,
                "lang": parser::lang_name(f.lang),
                "requested_mode": f.requested_mode,
                "effective_mode": f.mode,
                "fallback": f.fallback.map(|fb| match fb {
                    Fallback::ParseErrors { ratio } => json!({"reason": fb.reason(), "error_ratio": ratio}),
                    _ => json!({"reason": fb.reason()}),
                }),
                "original_bytes": f.original_bytes,
                "original_tokens": f.original_tokens,
                "compressed_bytes": f.compressed_bytes,
                "compressed_tokens": f.compressed_tokens,
                "redactions": f.redactions,
            })
        })
        .collect();
    let skipped: Vec<_> = r
        .skipped
        .iter()
        .map(|(path, skip)| json!({"path": path, "reason": skip.reason()}))
        .collect();
    let mut doc = json!({
        "schema": 1,
        "tokenizer": r.tokenizer,
        "bundle_tokens": r.bundle_tokens,
        "elapsed_ms": r.elapsed.as_millis() as u64,
        "files": files,
        "skipped": skipped,
    });
    if let Some(limit) = r.budget {
        doc["budget"] = json!({"limit": limit, "exceeded": r.bundle_tokens > limit});
    }
    let mut out = serde_json::to_string_pretty(&doc).expect("a JSON value always serializes");
    out.push('\n');
    out
}

const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";

struct Row {
    file: String,
    lang: &'static str,
    mode: String,
    bytes: (String, String),
    tokens: (String, String),
    saved: String,
    redactions: String,
}

impl Row {
    fn new(
        file: String,
        lang: &'static str,
        mode: String,
        bytes: (u64, u64),
        tokens: (usize, usize),
        redactions: usize,
    ) -> Row {
        Row {
            file,
            lang,
            mode,
            bytes: (human_bytes(bytes.0), human_bytes(bytes.1)),
            tokens: (thousands(tokens.0), thousands(tokens.1)),
            saved: saved(tokens.0, tokens.1),
            redactions: thousands(redactions),
        }
    }
}

pub fn render_table(r: &Report, color: bool) -> String {
    let paint = |code: &str, text: &str| {
        if color {
            format!("{code}{text}{RESET}")
        } else {
            text.to_string()
        }
    };

    let rows: Vec<Row> = r
        .files
        .iter()
        .map(|f| {
            Row::new(
                f.display.clone(),
                parser::lang_name(f.lang),
                match f.fallback {
                    Some(_) => format!("{}*", f.mode),
                    None => f.mode.to_string(),
                },
                (f.original_bytes, f.compressed_bytes),
                (f.original_tokens, f.compressed_tokens),
                f.redactions,
            )
        })
        .collect();
    let sum_bytes = |field: fn(&FileStat) -> u64| r.files.iter().map(field).sum();
    let sum = |field: fn(&FileStat) -> usize| r.files.iter().map(field).sum();
    let redactions = sum(|f| f.redactions);
    let totals = Row::new(
        counted(r.files.len(), "file", "files"),
        "",
        String::new(),
        (
            sum_bytes(|f| f.original_bytes),
            sum_bytes(|f| f.compressed_bytes),
        ),
        (sum(|f| f.original_tokens), sum(|f| f.compressed_tokens)),
        redactions,
    );

    let all = || rows.iter().chain(std::iter::once(&totals));
    let width =
        |cell: fn(&Row) -> usize, header: &str| all().map(cell).fold(header.len(), usize::max);
    let file_w = rows
        .iter()
        .map(|row| row.file.chars().count())
        .fold(4, usize::max);
    let lang_w = width(|row| row.lang.len(), "lang");
    let mode_w = width(|row| row.mode.len(), "mode");
    let bytes_w = (
        width(|row| row.bytes.0.len(), ""),
        width(|row| row.bytes.1.len(), ""),
    );
    let tokens_w = (
        width(|row| row.tokens.0.len(), ""),
        width(|row| row.tokens.1.len(), ""),
    );
    let bytes_cell = (bytes_w.0 + 3 + bytes_w.1).max("bytes".len());
    let tokens_cell = (tokens_w.0 + 3 + tokens_w.1).max("tokens".len());
    let saved_w = width(|row| row.saved.len(), "saved");
    let redactions_w = width(|row| row.redactions.len(), "redactions");
    let redactions_cell = |cell: &str| {
        if redactions > 0 {
            format!("  {cell:>redactions_w$}")
        } else {
            String::new()
        }
    };

    let pair = |(a, b): &(String, String), (wa, wb): (usize, usize), cell: usize| {
        format!("{:<cell$}", format!("{a:>wa$} → {b:<wb$}"))
    };
    let lead = |file: &str, lang: &str, mode: &str| {
        format!(" {file:<file_w$}  {lang:<lang_w$}  {mode:<mode_w$}  ")
    };

    let mut out = String::new();
    let header = format!(
        "{}{:<bytes_cell$}  {:<tokens_cell$}  {:>saved_w$}{}",
        lead("file", "lang", "mode"),
        "bytes",
        "tokens",
        "saved",
        redactions_cell("redactions")
    );
    out.push_str(&paint(BOLD, &header));
    out.push('\n');
    for row in &rows {
        let saved = format!("{:>saved_w$}", row.saved);
        out.push_str(&format!(
            "{}{}  {}  {}{}\n",
            lead(&row.file, row.lang, &row.mode),
            pair(&row.bytes, bytes_w, bytes_cell),
            pair(&row.tokens, tokens_w, tokens_cell),
            paint(GREEN, &saved),
            redactions_cell(&row.redactions)
        ));
    }
    let line_w = header.chars().count() - 1;
    out.push_str(&format!(" {}\n", paint(DIM, &"─".repeat(line_w))));
    let label_w = file_w + 2 + lang_w + 2 + mode_w;
    out.push_str(&format!(
        " {:<label_w$}  {}  {}  {}{}\n",
        totals.file,
        pair(&totals.bytes, bytes_w, bytes_cell),
        pair(&totals.tokens, tokens_w, tokens_cell),
        paint(GREEN, &format!("{:>saved_w$}", totals.saved)),
        redactions_cell(&totals.redactions)
    ));
    let mut footer = format!(
        "bundle: {} tokens ({})",
        thousands(r.bundle_tokens),
        r.tokenizer
    );
    if !r.skipped.is_empty() {
        footer.push_str(&format!(
            " · {} skipped ({})",
            r.skipped.len(),
            skip_counts(&r.skipped)
        ));
    }
    if redactions > 0 {
        footer.push_str(&format!(
            " · {}",
            counted(redactions, "redaction", "redactions")
        ));
    }
    footer.push_str(&format!(" · {} ms", r.elapsed.as_millis()));
    out.push_str(&format!(" {}\n", paint(DIM, &footer)));
    let mut notes: Vec<&str> = r
        .files
        .iter()
        .filter_map(|f| f.fallback)
        .map(|fb| fb.note())
        .collect();
    notes.sort_unstable();
    notes.dedup();
    for note in notes {
        out.push_str(&format!(" * {note}\n"));
    }
    out
}

/// Only counts: naming secret files would draw attention to them.
fn skip_counts(skipped: &[(String, Skip)]) -> String {
    let count = |reason: fn(&Skip) -> bool| skipped.iter().filter(|(_, s)| reason(s)).count();
    [
        (count(|s| *s == Skip::Excluded), "secret", "secrets"),
        (count(|s| *s == Skip::Binary), "binary", "binary"),
        (
            count(|s| matches!(s, Skip::TooLarge { .. })),
            "too large",
            "too large",
        ),
        (
            count(|s| matches!(s, Skip::Unreadable(_))),
            "unreadable",
            "unreadable",
        ),
    ]
    .into_iter()
    .filter(|&(n, _, _)| n > 0)
    .map(|(n, one, many)| counted(n, one, many))
    .collect::<Vec<_>>()
    .join(", ")
}

fn counted(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", thousands(n), if n == 1 { one } else { many })
}

fn human_bytes(n: u64) -> String {
    const KIB: f64 = 1024.0;
    match n {
        0..1024 => format!("{n}B"),
        1024..1_048_576 => format!("{:.1}K", n as f64 / KIB),
        _ => format!("{:.1}M", n as f64 / (KIB * KIB)),
    }
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(d);
    }
    out
}

fn saved(original: usize, compressed: usize) -> String {
    if original == 0 {
        return "0.0%".to_string();
    }
    format!(
        "{:.1}%",
        (1.0 - compressed as f64 / original as f64) * 100.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stat(display: &str, bytes: (u64, u64), tokens: (usize, usize)) -> FileStat {
        FileStat {
            display: display.to_string(),
            lang: Some(Language::Rust),
            requested_mode: "strip",
            mode: "strip",
            fallback: None,
            original_bytes: bytes.0,
            original_tokens: tokens.0,
            compressed_bytes: bytes.1,
            compressed_tokens: tokens.1,
            redactions: 0,
        }
    }

    fn report(files: Vec<FileStat>) -> Report {
        Report {
            files,
            skipped: Vec::new(),
            bundle_tokens: 1834,
            tokenizer: "o200k_base",
            budget: None,
            elapsed: Duration::from_millis(41),
        }
    }

    #[test]
    fn lays_out_per_file_rows_totals_and_a_footer() {
        let r = report(vec![
            stat("src/lib.rs", (14541, 3174), (3402, 912)),
            stat("src/parser/mod.rs", (12083, 2970), (2977, 870)),
        ]);

        assert_eq!(
            render_table(&r, false),
            format!(
                " file               lang  mode   bytes         tokens         saved\n \
                 src/lib.rs         rust  strip  14.2K → 3.1K  3,402 → 912    73.2%\n \
                 src/parser/mod.rs  rust  strip  11.8K → 2.9K  2,977 → 870    70.8%\n \
                 {}\n \
                 2 files                         26.0K → 6.0K  6,379 → 1,782  72.1%\n \
                 bundle: 1,834 tokens (o200k_base) · 41 ms\n",
                "─".repeat(66)
            )
        );
    }

    #[test]
    fn small_and_empty_files_show_bytes_and_no_saving() {
        let mut empty = stat("e.txt", (0, 0), (0, 0));
        empty.lang = None;
        let r = report(vec![empty, stat("a.rs", (512, 512), (9, 9))]);

        let table = render_table(&r, false);

        assert!(
            table.contains(" e.txt  text  strip    0B → 0B    0 → 0    0.0%\n"),
            "{table}"
        );
        assert!(
            table.contains(" a.rs   rust  strip  512B → 512B  9 → 9    0.0%\n"),
            "{table}"
        );
    }

    #[test]
    fn colour_codes_appear_only_when_asked_for() {
        let r = report(vec![stat("a.rs", (10, 5), (4, 2))]);

        assert!(!render_table(&r, false).contains('\x1b'));
        assert!(render_table(&r, true).contains("\x1b["));
    }

    #[test]
    fn the_footer_counts_skipped_files_per_reason_in_a_fixed_order() {
        let mut r = report(vec![stat("a.rs", (10, 10), (4, 4))]);
        r.skipped = vec![
            ("big.bin".to_string(), Skip::TooLarge { len: 5_000_000 }),
            (
                "locked.rs".to_string(),
                Skip::Unreadable(std::io::ErrorKind::PermissionDenied),
            ),
            ("logo.png".to_string(), Skip::Binary),
            (".env".to_string(), Skip::Excluded),
            ("id_rsa".to_string(), Skip::Excluded),
            ("font.woff".to_string(), Skip::Binary),
        ];

        let table = render_table(&r, false);

        assert!(
            table.ends_with(
                " bundle: 1,834 tokens (o200k_base) · 6 skipped \
                 (2 secrets, 2 binary, 1 too large, 1 unreadable) · 41 ms\n"
            ),
            "{table}"
        );
    }

    #[test]
    fn the_footer_counts_one_secret_in_the_singular_and_omits_absent_reasons() {
        let mut r = report(vec![stat("a.rs", (10, 10), (4, 4))]);
        r.skipped = vec![(".env".to_string(), Skip::Excluded)];

        let table = render_table(&r, false);

        assert!(
            table.ends_with(" bundle: 1,834 tokens (o200k_base) · 1 skipped (1 secret) · 41 ms\n"),
            "{table}"
        );
    }

    #[test]
    fn redactions_add_a_column_and_a_footer_count() {
        let mut redacted = stat("a.rs", (10, 10), (4, 4));
        redacted.redactions = 1;
        let r = report(vec![redacted, stat("b.rs", (10, 10), (4, 4))]);

        let table = render_table(&r, false);

        assert!(
            table.starts_with(
                " file  lang  mode   bytes      tokens  saved  redactions\n \
                 a.rs  rust  strip  10B → 10B  4 → 4    0.0%           1\n \
                 b.rs  rust  strip  10B → 10B  4 → 4    0.0%           0\n"
            ),
            "{table}"
        );
        assert!(
            table.ends_with(" bundle: 1,834 tokens (o200k_base) · 1 redaction · 41 ms\n"),
            "{table}"
        );
    }
}
