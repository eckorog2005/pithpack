# pithpack

Compress codebases into token-lean context bundles for LLMs.

> Pre-release: the design is in [`docs/SDD.md`](docs/SDD.md); vocabulary in [`GLOSSARY.md`](GLOSSARY.md).

## Example

Bundling this repo's own `src/` (output of `pithpack src`; the Bundle goes to stdout, the Report to stderr):

```text
 file                       lang  mode    bytes          tokens           saved  redactions
 src/cli.rs                 rust  strip*   5.5K → 5.5K    1,488 → 1,488    0.0%           0
 …
 src/lib.rs                 rust  strip*   6.0K → 5.8K    1,360 → 1,316    3.2%           0
 …
 src/redact/mod.rs          rust  strip*  10.2K → 10.1K   2,930 → 2,914    0.5%           7
 …
 src/sink.rs                rust  strip*   4.2K → 3.9K    1,077 → 1,004    6.8%           0
 ──────────────────────────────────────────────────────────────────────────────────────────
 19 files                                 82.6K → 81.3K  21,860 → 21,563   1.4%           7
 bundle: 22,173 tokens (o200k_base) · 7 redactions · 93 ms
 * skeleton and outline are not built yet
```

Only `strip` (remove Free Comments and redundant whitespace) is built so far, so the default `skeleton` compresses as `strip` and the Report marks it `strip*`. This repo's code carries few comments, so `strip` saves little here; collapsing function bodies in `skeleton` is where the design's roughly 30–70% fewer tokens per file comes from. See [`docs/SDD.md`](docs/SDD.md).

## Install

Once released: `cargo install pithpack`.

## License

Licensed under the [MIT license](LICENSE).
