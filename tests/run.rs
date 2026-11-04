mod common;

use common::{fixture, run, run_with_tty};

#[test]
fn bundles_a_directory_into_file_blocks_carrying_token_counts() {
    let dir = fixture(&[("src/lib.rs", "hello world\n")]);

    let out = run(dir.path(), &["src", "--no-tree"]);

    assert!(out.outcome.is_ok());
    let (header, body) = out.stdout.split_once('\n').unwrap();
    assert!(
        header.starts_with(&format!(
            "<bundle tool=\"pithpack\" version=\"{}\" mode=\"skeleton\" tokenizer=\"o200k_base\" files=\"1\" tokens=\"",
            env!("CARGO_PKG_VERSION")
        )),
        "{header}"
    );
    assert_eq!(
        body,
        "<file path=\"src/lib.rs\" lang=\"rust\" mode=\"strip\" tokens=\"3\">\n\
         hello world\n\
         </file>\n\
         </bundle>\n"
    );
}

fn bundle_tokens(bundle: &str) -> usize {
    let header = bundle.lines().next().unwrap();
    let (_, rest) = header.split_once(" tokens=\"").unwrap();
    rest.split('"').next().unwrap().parse().unwrap()
}

#[test]
fn the_bundle_header_counts_the_whole_rendered_bundle() {
    let dir = fixture(&[("a.txt", "hello world\n")]);

    let with_tree = bundle_tokens(&run(dir.path(), &["a.txt"]).stdout);
    let without_tree = bundle_tokens(&run(dir.path(), &["a.txt", "--no-tree"]).stdout);

    assert!(without_tree > 3, "wrappers count: {without_tree}");
    assert!(with_tree > without_tree, "{with_tree} vs {without_tree}");
}

#[test]
fn the_tokenizer_flag_switches_to_cl100k_and_names_it() {
    let dir = fixture(&[("a.txt", "hello world\n")]);

    let out = run(dir.path(), &["a.txt", "--tokenizer", "cl100k"]);

    let header = out.stdout.lines().next().unwrap();
    assert!(header.contains(" tokenizer=\"cl100k_base\" "), "{header}");
    assert!(
        out.stdout
            .contains("<file path=\"a.txt\" lang=\"text\" mode=\"strip\" tokens=\"3\">")
    );
}

#[test]
fn special_tokens_in_content_are_counted_as_plain_text() {
    let dir = fixture(&[("a.txt", "<|endoftext|>")]);

    let out = run(dir.path(), &["a.txt", "--no-tree"]);

    assert!(out.outcome.is_ok());
    assert!(!out.stdout.contains(" tokens=\"1\">"), "{}", out.stdout);
}

#[test]
fn every_bundle_starts_with_a_file_tree() {
    let dir = fixture(&[("src/lib.rs", "a\n"), ("src/x/mod.rs", "b\n")]);

    let out = run(dir.path(), &["src"]);

    let after_header = out.stdout.split_once('\n').unwrap().1;
    assert!(
        after_header.starts_with("<tree>\nsrc/\n  lib.rs\n  x/\n    mod.rs\n</tree>\n<file "),
        "{}",
        out.stdout
    );
}

fn file_paths(bundle: &str) -> Vec<&str> {
    bundle
        .lines()
        .filter_map(|l| l.strip_prefix("<file path=\""))
        .map(|l| l.split('"').next().unwrap())
        .collect()
}

#[test]
fn several_paths_combine_into_one_bundle_ordered_by_display_path() {
    let dir = fixture(&[
        ("src/z.rs", ""),
        ("src/a/b.rs", ""),
        ("Cargo.toml", ""),
        ("docs/x.md", ""),
    ]);

    let out = run(dir.path(), &["src", "./Cargo.toml", "src/a"]);

    assert_eq!(
        file_paths(&out.stdout),
        ["Cargo.toml", "src/a/b.rs", "src/z.rs"]
    );
    assert!(out.stdout.starts_with("<bundle ") && out.stdout.contains(" files=\"3\" "));
}

#[test]
fn ignore_files_are_respected_and_hidden_files_walked_but_git_never_included() {
    let dir = fixture(&[
        (".gitignore", "target/\n"),
        (".ignore", "*.log\n"),
        (".github/workflows/ci.yml", ""),
        (".git/config", ""),
        (".git/HEAD", ""),
        ("target/debug/out", ""),
        ("build.log", ""),
        ("src/main.rs", ""),
    ]);

    let out = run(dir.path(), &["."]);

    assert_eq!(
        file_paths(&out.stdout),
        [
            ".github/workflows/ci.yml",
            ".gitignore",
            ".ignore",
            "src/main.rs"
        ]
    );
}

#[test]
fn content_is_not_xml_escaped_except_a_closing_file_tag() {
    let dir = fixture(&[("t.ts", "if (a < b && c > d) { s = \"</file>\"; }")]);

    let out = run(dir.path(), &["t.ts", "--no-tree"]);

    assert!(
        out.stdout
            .contains("<file path=\"t.ts\" lang=\"typescript\" mode=\"strip\" tokens=\""),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(
            "\">\n\
             if (a < b && c > d) { s = \"<\\/file>\"; }\n\
             </file>\n"
        ),
        "{}",
        out.stdout
    );
}

#[test]
fn display_paths_use_forward_slashes_whatever_the_argument_separator() {
    let dir = fixture(&[("src/a/b/c.rs", "")]);
    let arg = std::path::Path::new(".").join("src").join("a");

    let out = run(dir.path(), &[arg.to_str().unwrap(), "--no-tree"]);

    assert_eq!(file_paths(&out.stdout), ["src/a/b/c.rs"]);
}

#[test]
fn an_absolute_argument_keeps_its_root_with_forward_slashes() {
    let dir = fixture(&[("src/a.rs", "")]);
    let arg = dir.path().join("src");

    let out = run(dir.path(), &[arg.to_str().unwrap(), "--no-tree"]);

    let paths = file_paths(&out.stdout);
    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with("/src/a.rs"), "{}", paths[0]);
    assert!(
        !paths[0].contains('\\') && !paths[0].contains("//"),
        "{}",
        paths[0]
    );
}

#[test]
fn the_report_table_goes_to_stderr_naming_each_file_and_the_bundle_total() {
    let dir = fixture(&[("src/lib.rs", "hello world\n")]);

    let out = run(dir.path(), &["src"]);

    let total = bundle_tokens(&out.stdout);
    assert!(
        out.stderr
            .contains(" src/lib.rs  rust  strip*  12B → 12B  3 → 3"),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr
            .contains(&format!(" bundle: {total} tokens (o200k_base) · ")),
        "{}",
        out.stderr
    );
    assert!(!out.stdout.contains("saved"), "{}", out.stdout);
}

#[test]
fn report_none_and_quiet_leave_stderr_empty() {
    let dir = fixture(&[("a.txt", "hello\n")]);

    assert_eq!(run(dir.path(), &["a.txt", "--report", "none"]).stderr, "");
    assert_eq!(run(dir.path(), &["a.txt", "-q"]).stderr, "");
}

#[test]
fn the_report_is_coloured_only_when_stderr_is_a_tty() {
    let dir = fixture(&[("a.txt", "hello\n")]);

    let plain = run_with_tty(dir.path(), &["a.txt"], false);
    let coloured = run_with_tty(dir.path(), &["a.txt"], true);

    assert!(!plain.stderr.is_empty() && !plain.stderr.contains('\x1b'));
    assert!(coloured.stderr.contains("\x1b["), "{}", coloured.stderr);
    assert!(!coloured.stdout.contains('\x1b'));
}

#[test]
fn the_report_counts_bytes_as_read_not_as_decoded() {
    let dir = fixture(&[]);
    std::fs::write(dir.path().join("a.txt"), b"\xff\xfe abc\n").unwrap();

    let out = run(dir.path(), &["a.txt"]);

    assert!(
        out.stderr.contains(" a.txt  text  strip*  7B → "),
        "{}",
        out.stderr
    );
}

#[test]
fn markdown_format_emits_a_heading_and_fenced_block_per_file() {
    let dir = fixture(&[("src/lib.rs", "pub fn a() {}\n")]);

    let out = run(dir.path(), &["src", "--format", "markdown"]);

    assert!(out.outcome.is_ok());
    assert!(
        out.stdout.starts_with("# pithpack bundle\n"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("\n## File tree\n\n```text\nsrc/\n  lib.rs\n```\n")
    );
    assert!(
        out.stdout.ends_with(
            "\n## src/lib.rs\n\nmode: skeleton · tokens: 5\n\n```rust\npub fn a() {}\n```\n"
        ),
        "{}",
        out.stdout
    );
}

#[test]
fn markdown_format_fences_files_that_contain_fences_with_a_longer_fence() {
    let dir = fixture(&[("README.md", "```sh\nls\n```\n")]);

    let out = run(dir.path(), &["README.md", "-f", "markdown", "--no-tree"]);

    assert!(
        out.stdout.contains("````text\n```sh\nls\n```\n````\n"),
        "{}",
        out.stdout
    );
}

#[test]
fn markdown_format_omits_the_tree_with_no_tree() {
    let dir = fixture(&[("a.txt", "x\n")]);

    let out = run(dir.path(), &["a.txt", "-f", "markdown", "--no-tree"]);

    assert!(!out.stdout.contains("File tree"));
}

#[test]
fn markdown_header_counts_the_whole_bundle_and_output_is_deterministic() {
    let dir = fixture(&[("a.txt", "hello world\n"), ("b.txt", "bye\n")]);

    let first = run(dir.path(), &[".", "-f", "markdown"]).stdout;
    let second = run(dir.path(), &[".", "-f", "markdown"]).stdout;

    assert_eq!(first, second);
    let tokens: usize = first
        .lines()
        .find_map(|l| l.strip_prefix("- tokens: "))
        .unwrap()
        .parse()
        .unwrap();
    assert!(tokens > 6, "wrappers count: {tokens}");
}
