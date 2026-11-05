#!/usr/bin/env bash
# Usage: bench/run.sh [hyperfine-json-out]
set -euo pipefail

for tool in cargo hyperfine jq; do
  command -v "$tool" >/dev/null || { echo "bench: required tool '$tool' not found" >&2; exit 1; }
done

root=$(cd "$(dirname "$0")/.." && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
json=${1:-$scratch/result.json}

cd "$root"
cargo build --release --locked --bin pithpack
cargo run --release --locked --example gen_fixture -- "$scratch/fixture"

hyperfine --warmup 3 --runs 20 --export-json "$json" \
  "$root/target/release/pithpack -1 -q $scratch/fixture"

# hyperfine 2.x nests timings under .summary.time_wall_clock; 1.x keeps them on the result.
line=$(jq -r '.results[0] | (.summary.time_wall_clock // .) | "mean: \(.mean * 1000 | round) ms (stddev \(.stddev * 1000 | round) ms)"' "$json")
echo "$line"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then echo "$line" >> "$GITHUB_STEP_SUMMARY"; fi
