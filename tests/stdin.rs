mod common;

use clap::Parser;
use common::{file_paths, fixture, run_with_stdin};
use pithpack::cli::CliArgs;
use pithpack::config::{self, ConfigError, Env};

#[test]
fn piped_stdin_without_paths_is_one_plain_text_source_file() {
    let dir = fixture(&[("src/lib.rs", "")]);

    let out = run_with_stdin(dir.path(), &["--no-tree"], "hi\n");

    assert!(out.outcome.is_ok());
    assert_eq!(file_paths(&out.stdout), ["&lt;stdin&gt;"]);
    assert!(
        out.stdout.contains(
            "<file path=\"&lt;stdin&gt;\" lang=\"text\" mode=\"strip\" tokens=\"2\">\n\
             hi\n\
             </file>\n"
        ),
        "{}",
        out.stdout
    );
    assert!(
        out.stderr.contains(" <stdin>  text  strip*  3B → 3B"),
        "{}",
        out.stderr
    );
}

#[test]
fn a_dash_argument_combines_stdin_with_walked_files_in_display_order() {
    let dir = fixture(&[("src/a.rs", "fn a() {}\n"), ("Cargo.toml", "")]);

    let out = run_with_stdin(dir.path(), &["src", "-", "Cargo.toml"], "diff\n");

    assert!(out.outcome.is_ok());
    assert_eq!(
        file_paths(&out.stdout),
        ["&lt;stdin&gt;", "Cargo.toml", "src/a.rs"]
    );
    assert!(out.stdout.contains(" files=\"3\" "));
    assert!(
        out.stdout.contains("\">\ndiff\n</file>\n"),
        "{}",
        out.stdout
    );
}

#[test]
fn stdin_is_read_once_however_many_dashes_are_given() {
    let dir = fixture(&[]);

    let out = run_with_stdin(dir.path(), &["-", "-", "--no-tree"], "once\n");

    assert_eq!(file_paths(&out.stdout), ["&lt;stdin&gt;"]);
}

#[test]
fn no_paths_with_a_terminal_on_stdin_is_an_error_instead_of_waiting_for_input() {
    let cli = CliArgs::try_parse_from(["pithpack"]).unwrap();
    let env = Env {
        cwd: std::env::temp_dir(),
        stdin_is_tty: true,
        config_dir: None,
    };

    let err = config::resolve(cli, &env).unwrap_err();

    assert!(matches!(err, ConfigError::NoInput), "{err:?}");
    assert!(err.to_string().contains("stdin is a terminal"), "{err}");
}

#[test]
fn lang_labels_stdin_in_the_bundle_and_the_report() {
    let dir = fixture(&[("a.txt", "")]);

    let out = run_with_stdin(dir.path(), &["-", "a.txt", "--lang", "python"], "x = 1\n");

    assert!(
        out.stdout
            .contains("<file path=\"&lt;stdin&gt;\" lang=\"python\" mode=\"strip\" "),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("<file path=\"a.txt\" lang=\"text\" mode=\"strip\" "),
        "--lang touches only stdin: {}",
        out.stdout
    );
    assert!(
        out.stderr.contains(" <stdin>  python  strip* "),
        "{}",
        out.stderr
    );
}

#[test]
fn an_unknown_lang_is_rejected_while_parsing_arguments() {
    let err = CliArgs::try_parse_from(["pithpack", "--lang", "cobol"]).unwrap_err();

    assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    assert!(
        err.to_string().contains("python"),
        "names the choices: {err}"
    );
}
