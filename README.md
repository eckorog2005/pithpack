# pithpack

Compress codebases into token-lean context bundles for LLMs.

> Pre-release: the design is in [`docs/SDD.md`](docs/SDD.md); vocabulary in [`GLOSSARY.md`](GLOSSARY.md).

## Example

Bundling this repo's own `src/` (output of `pithpack src`; the Bundle goes to stdout, the Report to stderr):

```text
 file                       lang  mode      bytes          tokens          saved  redactions
 src/cli.rs                 rust  skeleton   5.5K → 4.2K    1,491 → 1,115  25.2%           0
 src/compressor.rs          rust  skeleton  14.2K → 2.0K    3,249 → 539    83.4%           0
 …
 src/lib.rs                 rust  skeleton   6.0K → 761B    1,380 → 204    85.2%           0
 …
 src/redact/mod.rs          rust  skeleton  10.2K → 2.3K    2,930 → 631    78.5%           7
 …
 src/sink.rs                rust  skeleton   4.2K → 951B    1,077 → 269    75.0%           0
 ───────────────────────────────────────────────────────────────────────────────────────────
 19 files                                   91.5K → 28.0K  23,763 → 7,812  67.1%           7
 bundle: 8,434 tokens (o200k_base) · 7 redactions · 522 ms
```

The default Mode, `skeleton`, keeps declarations, signatures, types and Doc Comments, and collapses multi-line function bodies to `{ /* … */ }`. It is built for Rust, Go, Python (bodies become `...`, after any docstring) and TypeScript/JavaScript (bodies become `{ … }`) so far; other languages get `strip` (remove Free Comments and redundant whitespace), marked `strip*` in the Report, and `outline` compresses as `skeleton`. See [`docs/SDD.md`](docs/SDD.md).

## Install

Once released: `cargo install pithpack`.

## License

Licensed under the [MIT license](LICENSE).
