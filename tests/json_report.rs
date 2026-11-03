mod common;

use common::{fixture, run, write};
use serde_json::{Value, json};

#[test]
fn the_json_report_has_a_stable_shape_on_stderr_only() {
    let dir = fixture(&[("a.rs", "fn a() {}\n"), (".env", "KEY=1\n")]);
    write(dir.path(), "logo.png", "\u{0}\u{0}");
    let out = run(dir.path(), &[".", "--report", "json"]);
    out.outcome.unwrap();
    assert!(!out.stdout.contains("\"schema\""));
    let mut v: Value = serde_json::from_str(&out.stderr).unwrap();

    assert!(v["bundle_tokens"].take().as_u64().unwrap() > 0);
    assert!(v["elapsed_ms"].take().is_u64());
    assert!(v["files"][0]["original_tokens"].take().as_u64().unwrap() > 0);
    assert!(v["files"][0]["compressed_tokens"].take().as_u64().unwrap() > 0);

    assert_eq!(
        v,
        json!({
            "schema": 1,
            "tokenizer": "o200k_base",
            "bundle_tokens": null,
            "elapsed_ms": null,
            "files": [{
                "path": "a.rs",
                "lang": "rust",
                "requested_mode": "skeleton",
                "effective_mode": "strip",
                "fallback": {"reason": "mode_not_built"},
                "original_bytes": 10,
                "original_tokens": null,
                "compressed_bytes": 10,
                "compressed_tokens": null,
                "redactions": 0,
            }],
            "skipped": [
                {"path": ".env", "reason": "excluded"},
                {"path": "logo.png", "reason": "binary"},
            ],
        })
    );
}

#[test]
fn oversized_files_are_skipped_with_the_too_large_reason() {
    let dir = fixture(&[("big.txt", "123456789")]);
    let out = run(
        dir.path(),
        &[".", "--report", "json", "--max-file-size", "5"],
    );
    let v: Value = serde_json::from_str(&out.stderr).unwrap();
    assert_eq!(
        v["skipped"],
        json!([{"path": "big.txt", "reason": "too_large"}])
    );
}

#[test]
fn redactions_are_counted_but_no_secret_value_appears() {
    let dir = fixture(&[
        ("k.txt", "key = \"AKIAIOSFODNN7EXAMPLE\"\n"),
        (".env", "TOKEN=hunter2hunter2\n"),
    ]);
    let out = run(dir.path(), &[".", "--report", "json"]);
    assert!(!out.stderr.contains("AKIAIOSFODNN7EXAMPLE"));
    assert!(!out.stderr.contains("hunter2"));
    let v: Value = serde_json::from_str(&out.stderr).unwrap();
    assert_eq!(v["files"][0]["redactions"], 1);
}

#[test]
fn an_exceeded_budget_keeps_stderr_valid_json() {
    let dir = fixture(&[("a.txt", "hello world\n")]);
    let out = run(dir.path(), &["a.txt", "--report", "json", "--budget", "1"]);

    assert!(matches!(out.outcome, Ok(pithpack::Outcome::OverBudget)));
    let v: Value = serde_json::from_str(&out.stderr).unwrap();
    assert_eq!(v["budget"]["limit"], 1);
    assert_eq!(v["budget"]["exceeded"], true);
}
