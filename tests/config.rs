mod common;

use std::path::Path;

use clap::Parser;
use common::{file_paths, fixture, write};
use pithpack::cli::CliArgs;
use pithpack::config::{self, ConfigError, Env, Options};
use pithpack::formatter::Format;

fn resolve_with(
    cwd: &Path,
    config_dir: Option<&Path>,
    args: &[&str],
) -> Result<Options, ConfigError> {
    let cli =
        CliArgs::try_parse_from(["pithpack"].into_iter().chain(args.iter().copied())).unwrap();
    config::resolve(
        cli,
        &Env {
            cwd: cwd.to_path_buf(),
            stdin_is_tty: false,
            config_dir: config_dir.map(Path::to_path_buf),
        },
    )
}

fn user_config(dir: &Path, text: &str) {
    write(dir, "pithpack/config.toml", text);
}

#[test]
fn cli_beats_project_beats_user_beats_defaults() {
    let user = fixture(&[]);
    let project = fixture(&[("pithpack.toml", "format = \"markdown\"\nbudget = 200\n")]);
    user_config(
        user.path(),
        "format = \"xml\"\nbudget = 100\ntokenizer = \"cl100k\"\n",
    );

    let opts = resolve_with(project.path(), Some(user.path()), &[]).unwrap();
    assert_eq!(opts.format, Format::Markdown);
    assert_eq!(opts.budget, Some(200));
    assert_eq!(format!("{:?}", opts.tokenizer), "Cl100k");

    let opts = resolve_with(project.path(), Some(user.path()), &["--budget", "300"]).unwrap();
    assert_eq!(opts.budget, Some(300));

    let opts = resolve_with(project.path(), None, &["--no-config"]).unwrap();
    assert_eq!(opts.format, Format::Xml);
    assert_eq!(opts.budget, None);
}

#[test]
fn lists_concatenate_across_layers_and_scalars_are_replaced() {
    let user = fixture(&[]);
    let project = fixture(&[(
        "pithpack.toml",
        "exclude = [\"b\"]\ninclude = [\"rs\"]\n[exclusion]\nextra = [\"two\"]\n",
    )]);
    user_config(
        user.path(),
        "exclude = [\"a\"]\ninclude = [\"py\"]\n[exclusion]\nextra = [\"one\"]\n",
    );

    let opts = resolve_with(project.path(), Some(user.path()), &["-e", "c"]).unwrap();

    assert_eq!(opts.filter.exclude, ["a", "b", "c"]);
    assert_eq!(opts.exclusion_extra, ["one", "two"]);
    assert_eq!(opts.filter.include, ["rs"]);
}

#[test]
fn project_discovery_walks_up_but_stops_at_the_git_root() {
    let root = fixture(&[
        ("pithpack.toml", "budget = 1\n"),
        ("repo/.git/HEAD", ""),
        ("repo/pkg/deep/f.rs", ""),
        ("plain/sub/f.rs", ""),
    ]);

    let inside_repo = root.path().join("repo/pkg/deep");
    assert_eq!(resolve_with(&inside_repo, None, &[]).unwrap().budget, None);

    write(&root.path().join("repo"), "pithpack.toml", "budget = 2\n");
    assert_eq!(
        resolve_with(&inside_repo, None, &[]).unwrap().budget,
        Some(2)
    );

    let no_repo = root.path().join("plain/sub");
    assert_eq!(resolve_with(&no_repo, None, &[]).unwrap().budget, Some(1));
}

#[test]
fn config_flag_replaces_discovery_and_no_config_ignores_everything() {
    let user = fixture(&[]);
    let project = fixture(&[
        ("pithpack.toml", "budget = 1\n"),
        ("other.toml", "tree = false\n"),
    ]);
    user_config(user.path(), "budget = 9\n");

    let opts = resolve_with(
        project.path(),
        Some(user.path()),
        &["--config", "other.toml"],
    )
    .unwrap();
    assert!(!opts.tree);
    assert_eq!(opts.budget, Some(9));

    let opts = resolve_with(project.path(), Some(user.path()), &["--no-config"]).unwrap();
    assert!(opts.tree);
    assert_eq!(opts.budget, None);

    let err = resolve_with(project.path(), None, &["--config", "missing.toml"]).unwrap_err();
    assert!(matches!(err, ConfigError::Read { .. }), "{err:?}");
}

#[test]
fn secret_weakening_and_unknown_keys_are_errors_naming_the_key() {
    for (text, key) in [
        ("redact = false\n", "redact"),
        ("allow_secret_files = true\n", "allow_secret_files"),
        ("[exclusion]\nremove = [\".env\"]\n", "remove"),
        ("[redaction]\nenabled = false\n", "enabled"),
        ("trea = true\n", "trea"),
    ] {
        let project = fixture(&[("pithpack.toml", text)]);
        let err = resolve_with(project.path(), None, &[]).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{err:?}");
        assert!(err.to_string().contains(key), "{key}: {err}");
    }
}

#[test]
fn bad_values_are_errors() {
    for text in [
        "format = \"yaml\"\n",
        "mode = \"zip\"\n",
        "max_file_size = \"1GiB\"\n",
        "[exclusion]\nextra = [\"[\"]\n",
    ] {
        let project = fixture(&[("pithpack.toml", text)]);
        let err = resolve_with(project.path(), None, &[]).unwrap_err();
        assert!(
            matches!(err, ConfigError::Invalid { .. }),
            "{text}: {err:?}"
        );
    }
}

#[test]
fn max_file_size_accepts_bytes_or_suffixed_text() {
    for (text, bytes) in [("2048", 2048), ("\"2KiB\"", 2048), ("\"1MiB\"", 1 << 20)] {
        let project = fixture(&[("pithpack.toml", &format!("max_file_size = {text}\n"))]);
        let opts = resolve_with(project.path(), None, &[]).unwrap();
        assert_eq!(opts.filter.max_file_size, bytes);
    }
}

#[test]
fn extra_exclusion_globs_are_excluded_and_reported() {
    let dir = fixture(&[
        (
            "pithpack.toml",
            "[exclusion]\nextra = [\"secrets/**\", \"*.vault\"]\n",
        ),
        ("a.rs", "fn a() {}\n"),
        ("secrets/b.rs", "fn b() {}\n"),
        ("c.vault", "x\n"),
    ]);
    let cli = CliArgs::try_parse_from(["pithpack", "--report", "json", "."]).unwrap();
    let opts = config::resolve(
        cli,
        &Env {
            cwd: dir.path().to_path_buf(),
            stdin_is_tty: false,
            config_dir: None,
        },
    )
    .unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    pithpack::run(
        opts,
        pithpack::Io {
            stdin: &mut std::io::empty(),
            stdout: &mut stdout,
            stderr: &mut stderr,
            stderr_is_tty: false,
        },
    )
    .unwrap();
    let bundle = String::from_utf8(stdout).unwrap();
    let report = String::from_utf8(stderr).unwrap();

    assert_eq!(file_paths(&bundle), ["a.rs", "pithpack.toml"]);
    assert!(
        report.contains("secrets/b.rs") && report.contains("c.vault"),
        "{report}"
    );
}

#[test]
fn extra_patterns_redact_with_their_kind() {
    let dir = fixture(&[
        (
            "pithpack.toml",
            "[redaction]\nextra_patterns = [{ kind = \"internal-token\", regex = \"itk_[A-Za-z0-9]{8}\" }]\n",
        ),
        ("a.txt", "token itk_ABCD1234 here\n"),
    ]);
    let cli = CliArgs::try_parse_from(["pithpack", "a.txt"]).unwrap();
    let opts = config::resolve(
        cli,
        &Env {
            cwd: dir.path().to_path_buf(),
            stdin_is_tty: false,
            config_dir: None,
        },
    )
    .unwrap();
    let mut stdout = Vec::new();
    pithpack::run(
        opts,
        pithpack::Io {
            stdin: &mut std::io::empty(),
            stdout: &mut stdout,
            stderr: &mut Vec::new(),
            stderr_is_tty: false,
        },
    )
    .unwrap();
    let bundle = String::from_utf8(stdout).unwrap();

    assert!(
        bundle.contains("token [REDACTED:internal-token] here"),
        "{bundle}"
    );
    assert!(!bundle.contains("itk_ABCD1234"));
}

#[test]
fn an_invalid_extra_pattern_regex_is_an_error_naming_the_pattern() {
    let project = fixture(&[(
        "pithpack.toml",
        "[redaction]\nextra_patterns = [{ kind = \"broken-kind\", regex = \"(\" }]\n",
    )]);

    let err = resolve_with(project.path(), None, &[]).unwrap_err();

    assert!(err.to_string().contains("broken-kind"), "{err}");
}

#[test]
fn init_writes_a_config_that_resolves_to_the_defaults() {
    let dir = fixture(&[]);
    let path = config::init(dir.path()).unwrap();
    assert_eq!(path, dir.path().join("pithpack.toml"));

    let with_file = resolve_with(dir.path(), None, &["."]).unwrap();
    let defaults = resolve_with(dir.path(), None, &[".", "--no-config"]).unwrap();
    assert_eq!(format!("{with_file:?}"), format!("{defaults:?}"));
}

#[test]
fn init_refuses_to_overwrite_an_existing_config() {
    let dir = fixture(&[("pithpack.toml", "tree = false\n")]);
    let err = config::init(dir.path()).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("pithpack.toml")).unwrap(),
        "tree = false\n"
    );
}

#[test]
fn mode_and_docs_come_from_config_and_the_cli_overrides_them() {
    use pithpack::cli::ModeArg;
    let project = fixture(&[("pithpack.toml", "mode = \"outline\"\ndocs = false\n")]);

    let opts = resolve_with(project.path(), None, &[]).unwrap();
    assert_eq!(opts.mode, ModeArg::Outline);
    assert!(!opts.docs);

    let opts = resolve_with(project.path(), None, &["-0"]).unwrap();
    assert_eq!(opts.mode, ModeArg::Strip);
    let opts = resolve_with(project.path(), None, &["--mode", "skeleton"]).unwrap();
    assert_eq!(opts.mode, ModeArg::Skeleton);

    let docs_on = fixture(&[("pithpack.toml", "docs = true\n")]);
    let opts = resolve_with(docs_on.path(), None, &["--no-docs"]).unwrap();
    assert!(!opts.docs);
}
