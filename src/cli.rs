use std::path::PathBuf;

use clap::Parser;

use crate::encoder::TokenizerKind;
use crate::formatter::Format;
use crate::parser::Language;
use crate::report::ReportFormat;

#[derive(clap::ValueEnum, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeArg {
    Strip,
    Skeleton,
    Outline,
}

impl ModeArg {
    pub fn name(self) -> &'static str {
        match self {
            ModeArg::Strip => "strip",
            ModeArg::Skeleton => "skeleton",
            ModeArg::Outline => "outline",
        }
    }
}

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// Write a commented `pithpack.toml` in the current directory
    Init,
}

#[derive(Parser, Debug)]
#[command(
    name = "pithpack",
    version,
    about,
    args_conflicts_with_subcommands = true
)]
pub struct CliArgs {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Files or directories to bundle; `-` reads stdin, as does giving none with stdin piped
    pub paths: Vec<PathBuf>,

    /// Keep only files with these extensions, e.g. `rs,toml`
    #[arg(short, long, value_delimiter = ',', value_name = "EXT,...")]
    pub include: Vec<String>,

    /// Skip files matching this gitignore-style glob, relative to each walked directory
    #[arg(short, long, value_name = "GLOB", value_parser = parse_exclude)]
    pub exclude: Vec<String>,

    /// Skip files larger than this many bytes; accepts `KiB` and `MiB` suffixes [default: 1MiB]
    #[arg(long, value_name = "SIZE", value_parser = parse_size)]
    pub max_file_size: Option<u64>,

    /// Follow symbolic links; Exclusion still applies to the link target
    #[arg(long)]
    pub follow_symlinks: bool,

    /// Admit a secret-looking file, but only one whose exact path is given as an argument
    #[arg(long)]
    pub allow_secret_file: bool,

    /// Disable Redaction of secret-looking values; always warns on stderr
    #[arg(long)]
    pub no_redact: bool,

    /// Language of stdin, for labelling; without it stdin is Plain Text
    #[arg(long, value_name = "LANG", value_parser = parse_lang)]
    pub lang: Option<Language>,

    /// Bundle format [default: xml]
    #[arg(short, long, value_enum)]
    pub format: Option<Format>,

    /// Compression Mode [default: skeleton]
    #[arg(
        short,
        long,
        value_enum,
        value_name = "MODE",
        conflicts_with_all = ["strip", "skeleton", "outline"]
    )]
    pub mode: Option<ModeArg>,

    /// Alias for `--mode strip`
    #[arg(short = '0', conflicts_with_all = ["skeleton", "outline"])]
    pub strip: bool,

    /// Alias for `--mode skeleton`
    #[arg(short = '1', conflicts_with = "outline")]
    pub skeleton: bool,

    /// Alias for `--mode outline`
    #[arg(short = '2')]
    pub outline: bool,

    /// Also drop Doc Comments
    #[arg(long)]
    pub no_docs: bool,

    /// Omit the File Tree from the Bundle
    #[arg(long)]
    pub no_tree: bool,

    /// Tokenizer used for token counts [default: o200k]
    #[arg(long, value_enum)]
    pub tokenizer: Option<TokenizerKind>,

    /// Report format on stderr [default: table]
    #[arg(long, value_enum)]
    pub report: Option<ReportFormat>,

    /// Warn and exit 3 when the Bundle exceeds this many tokens
    #[arg(long, value_name = "TOKENS")]
    pub budget: Option<usize>,

    /// Write the Bundle to this file instead of stdout
    #[arg(short, long, value_name = "FILE", conflicts_with = "clipboard")]
    pub output: Option<PathBuf>,

    /// Copy the Bundle to the clipboard instead of stdout
    #[arg(short = 'c', long, visible_alias = "clip")]
    pub clipboard: bool,

    /// Use this project config instead of searching for `pithpack.toml`
    #[arg(long, value_name = "FILE", conflicts_with = "no_config")]
    pub config: Option<PathBuf>,

    /// Ignore all config files
    #[arg(long)]
    pub no_config: bool,

    /// Same as `--report none`
    #[arg(short, long)]
    pub quiet: bool,
}

fn parse_exclude(glob: &str) -> Result<String, String> {
    ignore::overrides::OverrideBuilder::new("")
        .add(&format!("!{glob}"))
        .map_err(|err| err.to_string())?;
    Ok(glob.to_string())
}

fn parse_lang(name: &str) -> Result<Language, String> {
    Language::from_name(name).ok_or_else(|| {
        let names: Vec<&str> = Language::ALL.iter().map(|lang| lang.name()).collect();
        format!("expected one of {}", names.join(", "))
    })
}

pub(crate) fn parse_size(size: &str) -> Result<u64, String> {
    let (digits, unit) = if let Some(n) = size.strip_suffix("MiB") {
        (n, 1024 * 1024)
    } else if let Some(n) = size.strip_suffix("KiB") {
        (n, 1024)
    } else {
        (size, 1)
    };
    digits
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(unit))
        .ok_or_else(|| {
            "expected a whole number of bytes, optionally with a KiB or MiB suffix".into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse_as_bytes_or_binary_suffixes() {
        assert_eq!(parse_size("0"), Ok(0));
        assert_eq!(parse_size("1500"), Ok(1500));
        assert_eq!(parse_size("512KiB"), Ok(524_288));
        assert_eq!(parse_size("1MiB"), Ok(1_048_576));
    }

    #[test]
    fn malformed_or_overflowing_sizes_are_rejected() {
        for size in [
            "",
            "MiB",
            "1 MiB",
            "1mib",
            "1GiB",
            "-1",
            "1.5KiB",
            "99999999999999999MiB",
        ] {
            assert!(parse_size(size).is_err(), "{size:?} should be rejected");
        }
    }
}
