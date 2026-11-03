use std::io::IsTerminal;
use std::path::{Component, Path, PathBuf};

use clap::ValueEnum;
use serde::Deserialize;

use crate::cli::{CliArgs, ModeArg, parse_size};
use crate::encoder::TokenizerKind;
use crate::formatter::Format;
use crate::parser::Language;
use crate::redact::{PatternSpec, Rules};
use crate::report::ReportFormat;
use crate::scanner::exclusion::Exclusion;
use crate::sink::SinkKind;

/// The process environment `resolve` depends on, injected so it can be faked in tests.
pub struct Env {
    pub cwd: PathBuf,
    pub stdin_is_tty: bool,
    /// The user's config directory; `pithpack/config.toml` inside it is the user config.
    pub config_dir: Option<PathBuf>,
}

impl Env {
    pub fn from_process() -> std::io::Result<Env> {
        let var = |name| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let config_dir = if cfg!(windows) {
            var("APPDATA")
        } else {
            var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|home| home.join(".config")))
        };
        Ok(Env {
            cwd: std::env::current_dir()?,
            stdin_is_tty: std::io::stdin().is_terminal(),
            config_dir,
        })
    }
}

#[derive(Debug)]
pub enum Input {
    Path {
        path: PathBuf,
        /// The argument as typed, normalised to `/` separators; prefixes every display path.
        display: String,
    },
    Stdin,
}

const DEFAULT_MAX_FILE_SIZE: u64 = 1024 * 1024;

#[derive(Debug)]
pub struct FilterOptions {
    /// Extensions without the leading dot; empty keeps every extension.
    pub include: Vec<String>,
    /// Gitignore-style globs, matched relative to each walked directory.
    pub exclude: Vec<String>,
    pub max_file_size: u64,
    pub follow_symlinks: bool,
}

#[derive(Debug)]
pub struct Options {
    pub inputs: Vec<Input>,
    pub filter: FilterOptions,
    pub allow_secret_file: bool,
    /// Globs added to the built-in Exclusion list.
    pub exclusion_extra: Vec<String>,
    pub redaction_extra: Vec<PatternSpec>,
    pub stdin_lang: Option<Language>,
    pub mode: ModeArg,
    pub docs: bool,
    /// Only the CLI can turn this off; no config key may.
    pub redact: bool,
    pub format: Format,
    pub tree: bool,
    pub tokenizer: TokenizerKind,
    pub report: ReportFormat,
    pub budget: Option<usize>,

    pub sink: SinkKind,
}

#[derive(Debug)]
pub enum ConfigError {
    /// A usage error: there are no paths, and reading a terminal would wait forever.
    NoInput,
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    Invalid {
        path: PathBuf,
        message: String,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::NoInput => f.write_str(
                "no paths given and stdin is a terminal; pass paths, or pipe input into pithpack",
            ),
            ConfigError::Read { path, source } => {
                write!(f, "cannot read config `{}`: {source}", path.display())
            }
            ConfigError::Invalid { path, message } => {
                write!(f, "invalid config `{}`: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Every key is optional. There is deliberately no key that disables Redaction or Exclusion
/// or admits secret files (ADR-0002), and unknown keys are errors so such a key can't be typo'd in.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    mode: Option<String>,
    format: Option<String>,
    tokenizer: Option<String>,
    tree: Option<bool>,
    docs: Option<bool>,
    include: Option<Vec<String>>,
    #[serde(default)]
    exclude: Vec<String>,
    max_file_size: Option<Size>,
    follow_symlinks: Option<bool>,
    budget: Option<usize>,
    #[serde(default)]
    exclusion: ExclusionSection,
    #[serde(default)]
    redaction: RedactionSection,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ExclusionSection {
    #[serde(default)]
    extra: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RedactionSection {
    #[serde(default)]
    extra_patterns: Vec<ExtraPattern>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExtraPattern {
    kind: String,
    regex: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Size {
    Bytes(u64),
    Text(String),
}

/// The lower layers folded together: scalars replaced, lists concatenated.
struct Settings {
    mode: ModeArg,
    docs: bool,
    format: Format,
    tokenizer: TokenizerKind,
    tree: bool,
    include: Vec<String>,
    exclude: Vec<String>,
    max_file_size: u64,
    follow_symlinks: bool,
    budget: Option<usize>,
    exclusion_extra: Vec<String>,
    redaction_extra: Vec<PatternSpec>,
}

impl Settings {
    fn builtin() -> Settings {
        Settings {
            mode: ModeArg::Skeleton,
            docs: true,
            format: Format::Xml,
            tokenizer: TokenizerKind::O200k,
            tree: true,
            include: Vec::new(),
            exclude: Vec::new(),
            max_file_size: DEFAULT_MAX_FILE_SIZE,
            follow_symlinks: false,
            budget: None,
            exclusion_extra: Vec::new(),
            redaction_extra: Vec::new(),
        }
    }

    fn apply(&mut self, cfg: FileConfig) -> Result<(), String> {
        fn parse<T: ValueEnum>(key: &str, value: &str) -> Result<T, String> {
            T::from_str(value, true).map_err(|_| format!("`{key}` has an unknown value `{value}`"))
        }
        if let Some(mode) = &cfg.mode {
            self.mode = parse("mode", mode)?;
        }
        if let Some(format) = &cfg.format {
            self.format = parse("format", format)?;
        }
        if let Some(tokenizer) = &cfg.tokenizer {
            self.tokenizer = parse("tokenizer", tokenizer)?;
        }
        if let Some(size) = cfg.max_file_size {
            self.max_file_size = match size {
                Size::Bytes(bytes) => bytes,
                Size::Text(text) => {
                    parse_size(&text).map_err(|err| format!("`max_file_size`: {err}"))?
                }
            };
        }
        for glob in &cfg.exclusion.extra {
            Exclusion::with_extra(std::slice::from_ref(glob))
                .map_err(|err| format!("exclusion glob `{glob}`: {err}"))?;
        }
        let patterns: Vec<PatternSpec> = cfg
            .redaction
            .extra_patterns
            .into_iter()
            .map(|p| PatternSpec {
                kind: p.kind,
                regex: p.regex,
            })
            .collect();
        Rules::with_extra(&patterns).map_err(|err| err.to_string())?;

        self.tree = cfg.tree.unwrap_or(self.tree);
        self.docs = cfg.docs.unwrap_or(self.docs);
        self.follow_symlinks = cfg.follow_symlinks.unwrap_or(self.follow_symlinks);
        self.budget = cfg.budget.or(self.budget);
        self.include = cfg.include.unwrap_or(std::mem::take(&mut self.include));
        self.exclude.extend(cfg.exclude);
        self.exclusion_extra.extend(cfg.exclusion.extra);
        self.redaction_extra.extend(patterns);
        Ok(())
    }
}

fn load(path: &Path) -> Result<FileConfig, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&text).map_err(|err| ConfigError::Invalid {
        path: path.to_path_buf(),
        message: err.to_string(),
    })
}

/// Searches `cwd` and its ancestors, stopping at the git root.
fn find_project_config(cwd: &Path) -> Option<PathBuf> {
    for dir in cwd.ancestors() {
        let candidate = dir.join("pithpack.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if dir.join(".git").exists() {
            break;
        }
    }
    None
}

fn config_layers(cli: &CliArgs, env: &Env) -> Result<Settings, ConfigError> {
    let mut settings = Settings::builtin();
    if cli.no_config {
        return Ok(settings);
    }
    let user = env
        .config_dir
        .as_ref()
        .map(|dir| dir.join("pithpack").join("config.toml"))
        .filter(|path| path.is_file());
    let project = match &cli.config {
        Some(path) => Some(env.cwd.join(path)),
        None => find_project_config(&env.cwd),
    };
    for path in user.into_iter().chain(project) {
        let cfg = load(&path)?;
        settings
            .apply(cfg)
            .map_err(|message| ConfigError::Invalid { path, message })?;
    }
    Ok(settings)
}

pub fn resolve(cli: CliArgs, env: &Env) -> Result<Options, ConfigError> {
    if cli.paths.is_empty() && env.stdin_is_tty {
        return Err(ConfigError::NoInput);
    }
    let settings = config_layers(&cli, env)?;
    let (dashes, paths): (Vec<_>, Vec<_>) =
        cli.paths.iter().partition(|arg| arg.as_os_str() == "-");
    let mut inputs: Vec<Input> = paths
        .into_iter()
        .map(|arg| Input::Path {
            path: env.cwd.join(arg),
            display: display_path(arg),
        })
        .collect();
    if cli.paths.is_empty() || !dashes.is_empty() {
        inputs.push(Input::Stdin);
    }
    let mut exclude = settings.exclude;
    exclude.extend(cli.exclude);
    let include = if cli.include.is_empty() {
        settings.include
    } else {
        cli.include
    };
    Ok(Options {
        inputs,
        filter: FilterOptions {
            include: include
                .iter()
                .map(|ext| ext.trim_start_matches('.').to_string())
                .filter(|ext| !ext.is_empty())
                .collect(),
            exclude,
            max_file_size: cli.max_file_size.unwrap_or(settings.max_file_size),
            follow_symlinks: cli.follow_symlinks || settings.follow_symlinks,
        },
        allow_secret_file: cli.allow_secret_file,
        exclusion_extra: settings.exclusion_extra,
        redaction_extra: settings.redaction_extra,
        stdin_lang: cli.lang,
        mode: match (cli.mode, cli.strip, cli.skeleton, cli.outline) {
            (Some(mode), ..) => mode,
            (None, true, ..) => ModeArg::Strip,
            (None, _, true, _) => ModeArg::Skeleton,
            (None, .., true) => ModeArg::Outline,
            _ => settings.mode,
        },
        docs: !cli.no_docs && settings.docs,
        redact: !cli.no_redact,
        format: cli.format.unwrap_or(settings.format),
        tree: !cli.no_tree && settings.tree,
        tokenizer: cli.tokenizer.unwrap_or(settings.tokenizer),
        budget: cli.budget.or(settings.budget),

        sink: match (cli.output, cli.clipboard) {
            (Some(path), _) => SinkKind::File(env.cwd.join(path)),
            (None, true) => SinkKind::Clipboard,
            (None, false) => SinkKind::Stdout,
        },
        report: if cli.quiet {
            ReportFormat::None
        } else {
            cli.report.unwrap_or(ReportFormat::Table)
        },
    })
}

const INIT_TEMPLATE: &str = r#"# pithpack.toml: every key is optional; values shown are the defaults.
# CLI flags override this file, which overrides the user config.
# `skeleton` and `outline` compress as `strip` until they are built.

mode            = "skeleton"   # "strip" | "skeleton" | "outline"
format          = "xml"        # "xml" | "markdown"
tokenizer       = "o200k"      # "o200k" | "cl100k"
tree            = true
docs            = true
max_file_size   = "1MiB"       # bytes, or with KiB/MiB suffix
follow_symlinks = false

# include = ["rs", "py", "ts"]                          # only these extensions; default is all
# exclude = ["target/**", "**/*.snap", "vendor/**"]     # gitignore-style globs
# budget  = 120000                                      # warn and exit 3 above this many tokens

# [exclusion]
# extra = ["*.tfstate", "secrets/**"]   # added to the built-in secret-file list

# [redaction]
# extra_patterns = [
#   { kind = "internal-token", regex = "itk_[A-Za-z0-9]{32}" },
# ]
"#;

/// Writes `pithpack.toml` into `dir`, refusing to touch an existing one.
pub fn init(dir: &Path) -> std::io::Result<PathBuf> {
    let path = dir.join("pithpack.toml");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    if let Err(err) = std::io::Write::write_all(&mut file, INIT_TEMPLATE.as_bytes()) {
        // A truncated file would block both re-running init and loading the config.
        let _ = std::fs::remove_file(&path);
        return Err(err);
    }
    Ok(path)
}

/// Joins `path`'s components with `/` on every OS, dropping `.` components.
pub(crate) fn display_path(path: &Path) -> String {
    let mut out = String::new();
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => out.push_str(&prefix.as_os_str().to_string_lossy()),
            Component::RootDir => out.push('/'),
            Component::CurDir => {}
            other => parts.push(other.as_os_str().to_string_lossy()),
        }
    }
    out.push_str(&parts.join("/"));
    out
}
