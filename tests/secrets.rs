//! The release-blocking secrets suite (SDD §7.2): no secret may reach the Bundle or the Report.

mod common;

use common::{fixture, run, run_with_stdin, write};
use serde_json::Value;

/// Real-shaped fake secrets, one or more per built-in Redaction rule, assembled at run time so
/// the source never holds a token that push protection would flag. Each entry is
/// `(kind, source line, substring that must never leak)`.
fn redaction_corpus() -> Vec<(&'static str, String, String)> {
    let token = |kind, parts: &[&str]| {
        let value = parts.concat();
        // `pub` so `outline` keeps the line instead of dropping it unredacted.
        (kind, format!("pub const T: &str = \"{value}\";\n"), value)
    };
    vec![
        token("aws-access-key", &["AKIA", "Z7QK2M4XW9TB3HNP"]),
        token("aws-access-key", &["ASIA", "Q2M4XW9TB3HNPZ7K"]),
        token(
            "github-token",
            &["ghp", "_A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8"],
        ),
        token(
            "github-token",
            &["ghs", "_Z9y8X7w6V5u4T3s2R1q0P9o8N7m6L5k4J3i2"],
        ),
        token(
            "github-token",
            &["github_pat", "_11ABCDEFG0123456789_abcdefghijABCDEF"],
        ),
        token(
            "anthropic-key",
            &["sk-", "ant-api03-Xy7Kq2Lm9Np4Rs8Tv3Wz6Ab1Cd5"],
        ),
        token("openai-key", &["sk-", "proj-Pq3Rs8Tv3Wz6Ab1Xy7Kq2Lm9"]),
        token("openai-key", &["sk-", "Lm9Np4Rs8Tv3Wz6Ab1Xy7Kq2"]),
        token(
            "google-api-key",
            &["AIza", "SyB7k2Lm9Np4Rs8Tv3Wz6Ab1Cd5Ef0Gh2Jk"],
        ),
        token("slack-token", &["xoxb", "-1234567890-abcdefABCDEF"]),
        token("stripe-key", &["sk_live", "_4eC39HqLyjWDarjtT1zdp7dc"]),
        token("stripe-key", &["rk_test", "_9dZp7dcT1zd4eC39HqLyjWDa"]),
        token(
            "jwt",
            &[
                "eyJhbGciOiJIUzI1NiJ9.",
                "eyJzdWIiOiIxMjM0In0.",
                "dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            ],
        ),
        (
            "private-key",
            [
                "-----BEGIN RSA PRIVATE ",
                "KEY-----\nMIIEowIBAAKCAQEA7d8Kq\n-----END RSA PRIVATE KEY-----\n",
            ]
            .concat(),
            "MIIEowIBAAKCAQEA7d8Kq".into(),
        ),
        (
            "url-credentials",
            "let db = \"postgres://admin:hunter2hunter2@db:5432/app\";\n".into(),
            "hunter2hunter2".into(),
        ),
        (
            "assigned-secret",
            "let api_token = \"q8Zr2Lx7Vm4Nt9Bw\";\n".into(),
            "q8Zr2Lx7Vm4Nt9Bw".into(),
        ),
    ]
}

/// One file name per built-in Exclusion glob.
const EXCLUDED_FILES: &[&str] = &[
    ".env",
    ".env.local",
    "prod.env",
    ".envrc",
    "server.pem",
    "tls.key",
    "cert.p12",
    "cert.pfx",
    "store.jks",
    "release.keystore",
    "ca.crt",
    "ca.cer",
    "ca.der",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "putty.ppk",
    ".npmrc",
    ".pypirc",
    ".netrc",
    ".git-credentials",
    ".docker/config.json",
    "credentials",
    "credentials.json",
    "aws-credentials-prod.json",
    "service-account-ci.json",
    "main.tfstate",
    "main.tfstate.backup",
    "vault.kdbx",
    ".htpasswd",
];

fn excluded_payload(i: usize) -> String {
    format!("excluded-payload-{i:02}-7qv")
}

fn redaction_source() -> String {
    redaction_corpus()
        .into_iter()
        .map(|(_, line, _)| line)
        .collect()
}

fn must_not_leak() -> Vec<String> {
    redaction_corpus()
        .into_iter()
        .map(|(_, _, secret)| secret)
        .chain((0..EXCLUDED_FILES.len()).map(excluded_payload))
        .collect()
}

fn assert_no_leak(args: &[&str], stdout: &str, stderr: &str) {
    for secret in must_not_leak() {
        assert!(
            !stdout.contains(&secret),
            "{args:?}: {secret} leaked into the Bundle"
        );
        assert!(
            !stderr.contains(&secret),
            "{args:?}: {secret} leaked into the Report"
        );
    }
}

fn assert_every_marker(args: &[&str], stdout: &str) {
    for (kind, _, _) in redaction_corpus() {
        assert!(
            stdout.contains(&format!("[REDACTED:{kind}]")),
            "{args:?}: no {kind} marker"
        );
    }
}

const MODE_FLAGS: &[&[&str]] = &[
    &[],
    &["-0"],
    &["-1"],
    &["-2"],
    &["--mode", "strip"],
    &["--mode", "skeleton"],
    &["--mode", "outline"],
    &["-1", "--no-docs"],
];

fn every_flag_combination(base: &[&'static str]) -> Vec<Vec<&'static str>> {
    let mut all = Vec::new();
    for format in ["xml", "markdown"] {
        for mode in MODE_FLAGS {
            for tree in [&[][..], &["--no-tree"]] {
                for report in ["table", "json"] {
                    let mut args = base.to_vec();
                    args.extend(["--format", format, "--report", report]);
                    args.extend(*mode);
                    args.extend(tree);
                    all.push(args);
                }
            }
        }
    }
    all
}

#[test]
fn no_secret_reaches_the_bundle_or_report_for_any_flag_combination() {
    let dir = fixture(&[
        ("src/keys.rs", &redaction_source()),
        ("notes.txt", &redaction_source()),
    ]);
    for (i, name) in EXCLUDED_FILES.iter().enumerate() {
        write(dir.path(), name, &excluded_payload(i));
    }

    for args in every_flag_combination(&["."]) {
        let out = run(dir.path(), &args);

        assert!(out.outcome.is_ok(), "{args:?}");
        assert_no_leak(&args, &out.stdout, &out.stderr);
        assert_every_marker(&args, &out.stdout);
        if args.contains(&"json") {
            let report: Value = serde_json::from_str(&out.stderr).unwrap();
            let excluded = report["skipped"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["reason"] == "excluded")
                .count();
            assert_eq!(excluded, EXCLUDED_FILES.len(), "{args:?}");
        }
    }
}

#[test]
fn no_secret_reaches_the_bundle_or_report_in_any_language_mode_and_format() {
    let dir = fixture(&[]);
    for ext in [
        "rs", "go", "py", "js", "jsx", "ts", "tsx", "html", "css", "txt",
    ] {
        write(dir.path(), &format!("src/keys.{ext}"), &redaction_source());
    }

    for format in ["xml", "markdown"] {
        for mode in ["strip", "skeleton", "outline"] {
            for report in ["table", "json"] {
                let args = [".", "--format", format, "--mode", mode, "--report", report];
                let out = run(dir.path(), &args);

                assert!(out.outcome.is_ok(), "{args:?}");
                assert_no_leak(&args, &out.stdout, &out.stderr);
            }
        }
    }
}

#[test]
fn no_secret_reaches_the_bundle_or_report_from_stdin() {
    let dir = fixture(&[]);

    for base in [&[][..], &["-"], &["-", "--lang", "rust"]] {
        for args in every_flag_combination(base) {
            let out = run_with_stdin(dir.path(), &args, &redaction_source());

            assert!(out.outcome.is_ok(), "{args:?}");
            assert_no_leak(&args, &out.stdout, &out.stderr);
            assert_every_marker(&args, &out.stdout);
        }
    }
}
