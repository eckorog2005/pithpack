# pithpack: Design

| | |
|---|---|
| Status | Implemented; first release pending (#18) |
| Numbering | Section numbers stay stable because issues and code cite them; §4 and §12 were removed |
| Date | 2026-10-27 |
| Package / binary | `pithpack` |
| Vocabulary | [`GLOSSARY.md`](../GLOSSARY.md); capitalised terms below (Source File, Mode, Bundle…) are defined there |
| Decisions | [`docs/adr/`](adr/) |

## 1. Purpose

Developers feeding code to LLMs pay for every token, and raw source is padded with Free Comments, whitespace, imports and implementation detail the model often doesn't need. `pithpack` walks a set of paths, drops what must never leave the machine, compresses each Source File according to a Mode, and emits one Bundle plus a Report of how many tokens were saved.

### 1.1 Goals

1. One static binary, zero runtime dependencies: grammars and BPE tables are compiled in; no network, no interpreter.
2. Syntax-aware compression for Rust, Python, TypeScript/JavaScript (incl. TSX/JSX) and Go; `strip`-only support for HTML and CSS; everything else is Plain Text.
3. No secrets in a Bundle by default (Exclusion + Redaction, §7).
4. Deterministic: same inputs and flags give a byte-identical Bundle.
5. `< 400 ms` for the reference workload (§9).

### 1.2 Non-goals (v1)

- Log compression (deduplication, timestamp normalisation): future work, §13.
- Structural compression of Markdown or other prose.
- Automatic Mode escalation to fit a token budget: `--budget` only warns (§5.4).
- Exact Claude token counts: Anthropic's tokenizer is not public; counts for Claude are estimates.
- Per-line token annotations.

## 2. Settled decisions

| # | Decision |
|---|---|
| D1 | Rust (edition 2024, MSRV 1.90), single binary crate, internal modules. |
| D2 | "Zero-dependency" means zero *runtime* dependencies; binary size (~15–25 MB) is the accepted cost. |
| D3 | Modes are `strip` (0), `skeleton` (1, default), `outline` (2); `-0/-1/-2` are aliases. |
| D4 | Doc Comments survive every Mode; only Free Comments are stripped ([ADR-0001](adr/0001-doc-comments-survive-every-mode.md)). `--no-docs` drops Doc Comments too. |
| D5 | Exclusion is not overridable by config or ignore files; Redaction is always on unless `--no-redact` ([ADR-0002](adr/0002-exclusion-cannot-be-overridden-by-config.md)). |
| D6 | The Bundle is the only thing on stdout; the Report goes to stderr. |
| D7 | Default format is XML tags; `--format markdown` is the alternative. |
| D8 | Token counts are per Source File and per Bundle. Default tokenizer `o200k_base`; `cl100k_base` optional. |
| D9 | HTML/CSS support `strip` only; higher Modes fall back with a Report note. |
| D10 | Every Bundle starts with a File Tree (`--no-tree` to omit). |
| D11 | Stdin is one Plain Text Source File named `<stdin>` unless `--lang` is given. |
| D12 | Symlinks are not followed by default. |
| D13 | Source Files whose parse is >10 % ERROR bytes fall back to `strip`. |

## 3. Architecture

### 3.1 Pipeline

```
 argv ──► cli ──► config::resolve ──► Options
                                         │
            ┌────────────────────────────┘
            ▼
        scanner ─────────► Vec<Candidate>   (walk, ignore files, Exclusion, filters,
            │                               size/binary checks; skips → Report)
            ▼
   ┌──── rayon par_iter, one task per Source File ─────────────────────┐
   │  read ─► redact ─► parser::parse ─► compressor::compress ─► encoder│
   │          (Redaction) (tree-sitter)   (Mode)              (tokens) │
   └──────────────────────────────┬─────────────────────────────────────┘
                                  ▼ collected and sorted by path
                             formatter ──► Bundle text ──► encoder (Bundle total)
                                  │
                         ┌────────┴────────┐
                         ▼                 ▼
                    sink (stdout │     report (stderr,
                    file │ clipboard)   table or JSON)
```

Every stage after `scanner` is a pure function of one Source File, which is what makes per-file parallelism, determinism and unit testing straightforward. Only `scanner`, `sink` and `main` touch the filesystem or the terminal.

### 3.2 Module responsibilities

| Module | Owns | Does not own |
|---|---|---|
| `cli` | clap definitions, flag → `CliArgs` | defaults, precedence |
| `config` | loading TOML layers, precedence, producing `Options` | Exclusion list (it can only *extend* it) |
| `scanner` | walking, ignore files, Exclusion, extension/glob filters, size and binary checks, symlink policy, stdin | reading content beyond the binary sniff |
| `redact` | secret patterns, entropy check, `[REDACTED:<kind>]` substitution | which files are read |
| `parser` | language detection, tree-sitter grammars, parsing | deciding what to keep |
| `compressor` | Mode semantics: the edit list each Mode produces, whitespace normalisation | tokenization |
| `encoder` | tokenizer selection, lazy BPE init, counting | formatting |
| `formatter` | XML/Markdown Bundle, File Tree, escaping | writing bytes anywhere |
| `report` | the Report model, table and JSON rendering | stdout |
| `sink` | stdout, `-o` file, clipboard | content |

The brief placed secret scrubbing in `scanner`. It is split into `redact` because `scanner` works on paths and `redact` works on content; keeping them apart lets each be tested without the other.

## 5. CLI specification

```
pithpack [OPTIONS] [PATHS]...
pithpack init                      # write a commented pithpack.toml in the current directory
```

With no `PATHS` and a non-TTY stdin, stdin is read. With no `PATHS` and a TTY stdin, pithpack exits 2 with a usage error rather than waiting for typed input. `-` reads stdin explicitly and may be combined with paths; giving it more than once still reads stdin once. Stdin is never subject to Exclusion, size or binary checks, which are about files.

### 5.1 Options

| Flag | Default | Meaning |
|---|---|---|
| `-m, --mode <strip\|skeleton\|outline>` | `skeleton` | Mode |
| `-0`, `-1`, `-2` | | aliases for the three Modes |
| `-f, --format <xml\|markdown>` | `xml` | Bundle format |
| `-o, --output <FILE>` | stdout | write the Bundle to a file |
| `-c, --clip` | off | copy the Bundle to the clipboard (stdout then stays empty) |
| `-i, --include <EXT,...>` | all | keep only these extensions |
| `-e, --exclude <GLOB>` (repeatable) | none | extra excludes |
| `--max-file-size <SIZE>` | `1MiB` | skip larger files |
| `--tokenizer <o200k\|cl100k>` | `o200k` | |
| `--lang <LANG>` | none | language for stdin |
| `--no-tree` | | omit the File Tree |
| `--no-docs` | | also drop Doc Comments |
| `--budget <N>` | none | warn and exit 3 if Bundle tokens exceed N |
| `--follow-symlinks` | off | follow links; Exclusion applies to the target |
| `--allow-secret-file` | off | admit an Excluded file *only if* its exact path is a CLI argument |
| `--no-redact` | off | disable Redaction (warns on stderr; CLI only) |
| `--report <table\|json\|none>` | `table` | Report format on stderr |
| `-q, --quiet` | | same as `--report none` |
| `--config <FILE>` | discovered | use this project config instead of discovery |
| `--no-config` | | ignore all config files |

### 5.2 Exit codes

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | runtime error (I/O, clipboard, unreadable config) |
| 2 | usage error (clap) |
| 3 | Bundle exceeded `--budget` (Bundle still written) |

### 5.3 Examples

```sh
pithpack ./src                       # skeleton, XML, to stdout
pithpack -2 -c .                     # outline of the repo to the clipboard
pithpack -0 -i rs,toml -o ctx.xml .  # strip, only Rust + TOML, to a file
git diff | pithpack -c               # diff as Plain Text, redacted, to clipboard
cat main.py | pithpack --lang python # stdin as a Python Source File
```

### 5.4 Budget

`--budget` compares against the token count of the *whole* Bundle text (tree and wrappers included), since that is what the model receives. Over budget: Bundle is still emitted, stderr gets `warning: bundle is 142,301 tokens, budget 100,000`, exit code 3.

## 6. Configuration

### 6.1 Layers

Highest wins: CLI flags → project `pithpack.toml` (searched from the cwd upwards, stopping at the git root) → user config (`$XDG_CONFIG_HOME/pithpack/config.toml`, macOS `~/.config/pithpack/config.toml`, Windows `%APPDATA%\pithpack\config.toml`) → built-in defaults. Lists (`exclude`, `exclusion.extra`, `redaction.extra_patterns`) are *concatenated* across layers; scalars are replaced.

### 6.2 Schema

```toml
# pithpack.toml: every key optional
mode            = "skeleton"   # "strip" | "skeleton" | "outline"
format          = "xml"        # "xml" | "markdown"
tokenizer       = "o200k"      # "o200k" | "cl100k"
tree            = true
docs            = true
include         = ["rs", "py", "ts"]
exclude         = ["target/**", "**/*.snap", "vendor/**"]
max_file_size   = "1MiB"       # bytes, or with KiB/MiB suffix
follow_symlinks = false
budget          = 120000

[exclusion]
extra = ["*.tfstate", "secrets/**"]   # added to the built-in list; there is no way to remove entries

[redaction]
extra_patterns = [
  { kind = "internal-token", regex = "itk_[A-Za-z0-9]{32}" },
]
```

There is deliberately no `redact = false`, `allow_secret_files` or `exclusion.remove` key (ADR-0002). Unknown keys are an error, so a typo can't be silently ignored.

## 7. Secret handling

### 7.1 Exclusion (`scanner::exclusion`)

Built-in globs, case-insensitive, each anchored as `**/<glob>` and matched against the whole path, so `.docker/config.json` is caught even when the walk starts inside `.docker/`:

```
.env  .env.*  *.env  .envrc
*.pem  *.key  *.p12  *.pfx  *.jks  *.keystore  *.crt  *.cer  *.der
id_rsa*  id_dsa*  id_ecdsa*  id_ed25519*  *.ppk
.npmrc  .pypirc  .netrc  .git-credentials  .docker/config.json
credentials  credentials.json  *credentials*.json  service-account*.json
*.tfstate  *.tfstate.backup  *.kdbx  .htpasswd
```

Exclusion is checked independently of ignore files and filters, and before a candidate is ever opened, so a `!` re-include in `.gitignore` or a matching `--include` cannot admit an Excluded file. Admission requires both conditions: the exact path is a CLI argument (not discovered by walking) *and* `--allow-secret-file` is set. Excluded files appear in the Report by count only ("3 skipped (3 secrets)"), and with paths under `--report json`.

### 7.2 Redaction (`redact`)

Runs on the raw text of every Source File (Plain Text included) before parsing, compression or tokenization, so no later stage ever holds the secret.

Rules are a `regex::RegexSet` pre-filter followed by per-rule capture:

| Kind | Shape |
|---|---|
| `aws-access-key` | `\b(AKIA\|ASIA)[0-9A-Z]{16}\b` |
| `github-token` | `\bgh[pousr]_[A-Za-z0-9]{36,}\b`, `github_pat_…` |
| `openai-key` | `\bsk-(proj-)?[A-Za-z0-9_-]{20,}\b` |
| `anthropic-key` | `\bsk-ant-[A-Za-z0-9_-]{20,}\b` |
| `google-api-key` | `\bAIza[0-9A-Za-z_-]{35}\b` |
| `slack-token` | `\bxox[abprs]-[A-Za-z0-9-]{10,}\b` |
| `stripe-key` | `\b(sk\|rk)_(live\|test)_[A-Za-z0-9]{20,}\b` |
| `jwt` | `\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b` |
| `private-key` | `-----BEGIN [A-Z ]*PRIVATE KEY-----` … `-----END …-----` (whole block) |
| `url-credentials` | `[a-z]+://[^/\s:@]+:[^/\s@]+@` (password part only) |
| `assigned-secret` | identifier matching `(?i)(secret\|token\|passw(or)?d\|api_?key\|auth)` assigned a string literal of ≥ 16 chars with Shannon entropy ≥ 3.5 bits/char |

Only the secret span is replaced, with `[REDACTED:<kind>]`; surrounding quotes and syntax are kept, so the file still parses. The Report shows a redaction count per Source File.

`tests/secrets.rs` runs every rule against a corpus of real-shaped fake secrets in every Mode and asserts that no secret substring appears in the Bundle, the Report, or the JSON Report.

## 8. Module behaviour

### 8.1 `scanner`

Implementation uses `ignore::WalkBuilder` (`.gitignore`, `.ignore`, global gitignore, and `.pithignore` via `add_custom_ignore_filename`) with `build_parallel()`. Hidden files are walked (so `.github/` is included) but `.git/` itself is always skipped. `-i` maps to `ignore` types and `-e` to `!`-prefixed overrides rooted at each walked directory, so neither can admit a file an ignore file hides. As in ripgrep, a file named directly on the command line bypasses ignore files and `-i`/`-e`; Exclusion, size and binary checks still apply to it, and its link target is always checked against Exclusion. Binary sniff reads the first 8 KiB and looks for a NUL byte.

### 8.2 `parser`

`tree_sitter::Parser` is not `Sync`; each rayon worker keeps one in a `thread_local!`. `compressor` holds the per-language knowledge as tree-sitter node kinds, so adding a language means adding a grammar and those rules.

Doc Comment classification per language:

| Language | Doc Comment |
|---|---|
| Rust | `///`, `//!`, `/** */`, `/*! */` |
| Python | first string-expression statement of a module, class or function body |
| TS/JS | `/** … */` immediately preceding a declaration |
| Go | contiguous `//` or `/* */` block ending on the line directly above a top-level declaration or a struct field |
| HTML/CSS | none: all comments are Free Comments |

### 8.3 `compressor`

All Modes work the same way: they produce a list of non-overlapping byte-range edits against the (redacted) source, apply it in a single pass, then normalise whitespace. Edits never touch bytes they don't cover, so the output keeps the original's formatting wherever it is retained.

**`strip`**: remove every Free Comment (and Doc Comment under `--no-docs`); remove trailing whitespace; collapse runs of blank lines to at most one; indentation is preserved (significant in Python, and cheap in `o200k_base`).

**`skeleton`**: `strip`, plus each function or method body whose source spans more than one line is replaced by the shortest placeholder that keeps the file parseable:

| Language | Placeholder |
|---|---|
| Rust, Go | `{ /* … */ }` (`{ … }` alone is an ERROR node) |
| TS/JS | `{ … }` |
| Python | `...` (Doc Comment, if kept, stays as the first line of the body) |

Only function and method bodies are collapsed. Containers (`impl`, `class`, `struct`, `interface`, `trait`, `enum`, Go `type … struct`) are kept, with their methods' bodies collapsed individually. Imports are kept.

**`outline`**: `skeleton`, plus drop every top-level or member item that isn't public, and drop imports except re-exports (`pub use`, `export … from`, `__all__`-listed re-imports).

Public means:

| Language | Rule |
|---|---|
| Rust | `pub` visibility only (`pub(crate)`, `pub(super)`, `pub(in …)` are private) |
| Go | identifier starts with an uppercase letter |
| TS/JS | has an `export` ancestor or modifier, or is a member of an exported class without `private` or `#` |
| Python | in `__all__` if the module defines one; otherwise the name has no leading `_` |

**Fallbacks**, shown in the Report:

- A strip-only language asked for `skeleton`/`outline` → `strip`.
- More than 10 % ERROR bytes → `strip` (comment removal still uses the partial tree; if the tree is unusable, Plain Text).
- An undetected language → Plain Text: Redaction, trailing-whitespace trim and blank-line collapse only.

### 8.4 `encoder`

Backed by `tiktoken-rs`, with special tokens counted as text. BPE construction is the largest fixed startup cost, so `lib::run` starts `Encoder::get` on a background thread before scanning begins and joins it once Source Files are read (§9.2).

### 8.5 `formatter`

XML Bundle:

```xml
<bundle tool="pithpack" version="0.3.0" mode="skeleton" tokenizer="o200k_base" files="2" tokens="1834">
<tree>
src/
  lib.rs
  parser/
    mod.rs
</tree>
<file path="src/lib.rs" lang="rust" mode="skeleton" tokens="912">
pub fn run(opts: Options, io: Io) -> Result<Outcome> { … }
</file>
<file path="src/parser/mod.rs" lang="rust" mode="skeleton" tokens="870">
…
</file>
</bundle>
```

- File content is **not** XML-escaped: LLMs read it as text, and escaping `<`/`&` in code costs tokens and readability. The one exception is a literal `</file>` inside content, which is written as `<\/file>` so a block can't close early; this is the only byte pithpack ever alters for formatting reasons, and it is documented.
- The Bundle-level `tokens` attribute counts the whole rendered Bundle. Because the number sits inside the text it counts, `lib::run` renders with `tokens: 0`, counts, then renders again with the count; the attribute's own digits can shift the true total by ±2 tokens, which is accepted.
- Markdown format uses a `## path` heading per file and a fence one backtick longer than the longest backtick run in that file (minimum 3), with the language as the info string.

### 8.6 `report`

Table (stderr):

```
 file                    lang    mode      bytes          tokens         saved  redactions
 src/lib.rs              rust    skeleton  14.2K → 3.1K   3,402 → 912    73.2%           2
 src/parser/mod.rs       rust    skeleton  11.8K → 2.9K   2,977 → 870    70.8%           0
 web/style.css           css     strip*     4.0K → 2.7K   1,105 → 744    32.7%           0
 ──────────────────────────────────────────────────────────────────────────────────────────
 3 files                                   30.0K → 8.7K   7,484 → 2,526  66.2%           2
 bundle: 2,610 tokens (o200k_base) · 4 skipped (1 secret, 2 binary, 1 too large) · 2 redactions · 41 ms
 * strip is the only Mode for this language
```

Original token counts are of the redacted text, so a secret is never tokenized. Colour is used only when stderr is a TTY. The JSON Report has a stable schema, versioned as `"schema": 1`.

### 8.7 `sink`

The clipboard uses `arboard`. On Linux (X11 and Wayland) the clipboard lives only as long as the owning process, so `sink` spawns a detached child that sets the clipboard with `SetExtLinux::wait()` and exits when another app takes ownership; the parent returns immediately. File output writes to a temporary file next to the target and renames it into place.

### 8.8 `lib::run`

`main` is the only place that maps `Outcome`/errors to exit codes, so `lib::run` can be tested in-process.

## 9. Performance

### 9.1 Target

Reference workload: `benches/gen_fixture.rs` deterministically generates 1,000 Source Files averaging 8 KiB, split evenly across Rust, Python, TypeScript and Go. Command: `pithpack -1 -q fixture/ > /dev/null`. Measured with `hyperfine --warmup 3` on an 8-core Apple Silicon machine with a warm filesystem cache. Target: mean **< 400 ms**. Tree-sitter parsing alone takes about 800 ms of CPU on this workload, which leaves too little of the original 200 ms target for everything else on a chip with four performance cores.

### 9.2 Strategy

- Parallel walk (`ignore` parallel walker) and per-file work on rayon.
- Encoder BPE initialisation runs on a background thread while the walk is in progress (this is the biggest single fixed cost, measured in tens of milliseconds).
- Redaction rules and Exclusion globs compiled once (`LazyLock`); parsers reused per thread.
- `Encoder::count` splits large text at newlines no tokenizer piece can span and counts the parts in parallel; the Bundle total reuses per-file counts through `count_with_known`.
- In `skeleton` and `outline`, the compressor does not walk an elided function body when the tree has no errors.
- `redact` returns `Cow::Borrowed` when nothing matches; `RegexSet` pre-filter avoids running individual rules on clean files.
- Files read with a single `read_to_end`; no mmap (more complexity than it is worth below 1 MiB).
- Release profile: `lto = "thin"`, `codegen-units = 1`.

### 9.3 Risk

If BPE initialisation alone breaks the budget, the fallback is to pre-build the rank hash map at compile time (a `build.rs` emitting a perfect-hash table) at the cost of binary size.

## 10. Testing

| Layer | Tooling | Covers |
|---|---|---|
| Unit | `#[cfg(test)]` | precedence, Exclusion globs, each redaction rule, edit-list application, public-item rules, fence selection, File Tree rendering |
| Snapshot | `insta` | each language × Mode → compressed text; reviewed in PRs |
| Parseability | in `tests/modes.rs` | every `skeleton`/`outline` output for a Full-support language must re-parse with zero ERROR nodes |
| End-to-end | `assert_cmd` | flags, exit codes, stdin, `-o`, stdout/stderr separation, determinism (run twice, compare bytes) |
| Secrets | `tests/secrets.rs` | no secret corpus string ever reaches stdout, stderr or JSON, in any Mode/format |
| Benchmark | `hyperfine` in `bench.yml` | trend only, never blocks a PR |

The clipboard is tested behind a `PITHPACK_CLIPBOARD=file:<path>` test hook so CI needs no display server.

## 11. Repository, CI and contribution flow

- **License**: `MIT`.
- **Merging**: squash only; the PR title (Conventional Commits) and body become the commit on `main`; branches are deleted on merge.
- **Ruleset on `main`**: PR required (0 approvals while there is a single maintainer; raise to 1 when a second joins), conversations resolved, required status check `ci-ok` with branch up to date, linear history, no force pushes, no deletion, no bypass actors.
- **CI (`ci.yml`)**: `fmt`, `clippy -D warnings`, `test` (ubuntu, macOS arm64, Windows, via `cargo nextest`) plus doctests, `msrv` (Rust 1.90), `deny` (`cargo-deny`), `pr-title` (semantic PR title). `ci-ok` aggregates them and is the only required check, so jobs can change without editing the ruleset.
- **Supply chain**: every action pinned to a full commit SHA; Dependabot weekly for `cargo` and `github-actions`, minor and patch updates grouped.
- **Releases**: `cargo-dist` generates `release.yml`. Targets: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`. Installers: shell, PowerShell, Homebrew tap; `cargo install pithpack`.

## 13. Future work

- **Log compression**: collapse repeated lines into `×N`, normalise timestamps and UUIDs, keep first and last occurrences.
- **Budget fitting**: automatically escalate Mode per Source File, largest first, until under `--budget`.
- **More languages**: Java, C#, C/C++, Ruby, Kotlin, Swift; mostly query-writing.
- **Markdown structure**: drop badges and HTML, collapse code fences in docs.
- **Claude token counts**: if Anthropic publishes a tokenizer, add it as a `--tokenizer` value.
- **Diff awareness**: parse unified diffs and apply language Modes to hunks.

## 14. Risks

| Risk | Mitigation |
|---|---|
| Grammar crates require a newer rustc than MSRV 1.90 | `msrv` job catches it; bump MSRV deliberately in its own PR |
| Redaction false negatives | extensible patterns, a corpus-based test suite, and Exclusion as the primary defence |
| Redaction false positives make code unreadable | typed markers keep the meaning; entropy threshold on generic assignments |
| tree-sitter grammar ABI drift between crates | pin grammar versions together; Dependabot groups them; snapshots catch output changes |
| Binary size growth from grammars | accepted (D2); track size in the release notes |
| `o200k_base` counts misrepresent Claude usage | label as estimates; offer `cl100k`; future tokenizer slot |
