# pithpack

Compress codebases into token-lean context bundles for LLMs.

pithpack walks a set of paths, drops what must never leave your machine, compresses each Source File, and emits one Bundle plus a Report of how many tokens that saved.

![Terminal demo: pithpack compresses a small Python file in skeleton and outline Modes, redacting its API key and reporting the tokens saved](demo/demo.gif)

## Install

No release has been published yet, so build from the repository (needs Rust 1.90 or newer):

```sh
cargo install --git https://github.com/eckorog2005/pithpack --locked
```

These routes will work from the first release:

```sh
# macOS and Linux
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/eckorog2005/pithpack/releases/latest/download/pithpack-installer.sh | sh

# Windows (PowerShell)
powershell -ExecutionPolicy Bypass -c "irm https://github.com/eckorog2005/pithpack/releases/latest/download/pithpack-installer.ps1 | iex"

# Homebrew
brew install eckorog2005/tap/pithpack

# crates.io
cargo install pithpack
```

## Quick start

```sh
pithpack ./src          # skeleton of ./src: Bundle on stdout, Report on stderr
pithpack -2 -c .        # outline of the whole repo, copied to the clipboard
git diff | pithpack -c  # a diff as Plain Text, redacted, copied to the clipboard
```

## Example

`pithpack ./src` on a small Python file with a hard-coded API key. The Bundle goes to stdout:

```xml
<bundle tool="pithpack" version="0.0.0" mode="skeleton" tokenizer="o200k_base" files="1" tokens="123">
<tree>
src/
  cart.py
</tree>
<file path="src/cart.py" lang="python" mode="skeleton" tokens="48">
"""Shopping cart totals."""

API_KEY = "[REDACTED:anthropic-key]"

def total(prices, tax=0.2):
    """Return the total including tax."""
    ...

def _audit(prices):
    ...
</file>
</bundle>
```

The Report goes to stderr:

```text
 file         lang    mode      bytes        tokens   saved  redactions
 src/cart.py  python  skeleton  343B → 174B  85 → 48  43.5%           1
 ──────────────────────────────────────────────────────────────────────
 1 file                         343B → 174B  85 → 48  43.5%           1
 bundle: 123 tokens (o200k_base) · 1 redaction · 86 ms
```

`-f markdown` writes the Bundle as Markdown, and `--report json` writes the Report as JSON.

## Modes

- **`strip`** (`-0`) removes Free Comments and redundant whitespace, and leaves all code in place.
- **`skeleton`** (`-1`, the default) also collapses multi-line function bodies, keeping declarations, signatures, types and Doc Comments.
- **`outline`** (`-2`) also drops private items, leaving only the public surface.

`skeleton` and `outline` cover Rust, Go, Python, TypeScript, TSX, JavaScript and JSX. HTML and CSS always get `strip`, other text only has its whitespace cleaned up, and the Report marks these fallbacks `strip*`. Doc Comments survive every Mode unless you pass `--no-docs`.

## Secrets

Secrets are handled in two stages, both on by default:

1. **Exclusion** skips secret-bearing files such as `.env`, `*.pem`, `id_rsa*` and `credentials.json` before they are opened. Ignore files and config cannot re-admit them. The Report counts them as skipped.
2. **Redaction** replaces secrets found inside the remaining files, such as API keys and tokens, with `[REDACTED:<kind>]` before any parsing or token counting.

Two flags turn these off, and only on the command line:

- `--allow-secret-file` admits an Excluded file, but only if its exact path is a command-line argument. Files found by walking a directory stay Excluded.
- `--no-redact` disables Redaction and prints a warning on stderr. Secrets in your files then reach the Bundle.

Redaction matches known secret shapes, so it can miss others. Review a Bundle before sending it somewhere you don't trust.

## Token counts

Token counts are estimates. They come from OpenAI's `o200k_base` tokenizer, or `cl100k_base` with `--tokenizer cl100k`. Anthropic's tokenizer is not public, so counts for Claude will differ. `--budget <N>` warns and exits with code 3 if the Bundle is over `N` by this estimate; the Bundle is still written.

## Performance

The target is a mean under 400 ms for `pithpack -1 -q` over the reference workload (1,000 Source Files averaging 8 KiB) on an 8-core Apple Silicon machine with a warm cache. The last measurement on an Apple M1 was 376 ms.

To reproduce the measurement, install `hyperfine` and `jq`, then run:

```sh
bench/run.sh
```

## Design docs

- [`GLOSSARY.md`](GLOSSARY.md) defines the vocabulary used here.
- [`docs/DESIGN.md`](docs/DESIGN.md) is the full design, including the CLI and configuration reference.
- [`docs/adr/`](docs/adr/) records the architecture decisions.

## License

Licensed under the [MIT license](LICENSE).
