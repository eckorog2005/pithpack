//! Writes the DESIGN.md §9.1 reference workload: `cargo run --release --example gen_fixture -- <out-dir>`.
use std::fs;
use std::io;
use std::path::Path;

const FILES_PER_LANG: usize = 250;
const FILES_PER_DIR: usize = 25;
const MIN_SIZE: usize = 6 * 1024;
const SIZE_SPAN: usize = 4 * 1024;
const SEED: u64 = 0x5eed_0f91;

const NOUNS: [&str; 12] = [
    "order", "user", "session", "token", "batch", "index", "cache", "queue", "frame", "route",
    "event", "ledger",
];
const VERBS: [&str; 10] = [
    "load", "merge", "parse", "update", "verify", "flush", "resolve", "encode", "filter", "commit",
];

struct Rng(u64);

impl Rng {
    // SplitMix64: fixed algorithm so output never depends on a library version.
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

struct Lang {
    dir: &'static str,
    ext: &'static str,
    header: &'static str,
    item: fn(noun: &str, verb: &str, n: usize) -> String,
}

const LANGS: [Lang; 4] = [
    Lang {
        dir: "rust",
        ext: "rs",
        header: "use std::collections::HashMap;\nuse std::fmt;\n\n",
        item: rust_item,
    },
    Lang {
        dir: "python",
        ext: "py",
        header: "import json\nimport logging\nfrom dataclasses import dataclass\nfrom typing import Optional\n\nlog = logging.getLogger(__name__)\n\n\n",
        item: python_item,
    },
    Lang {
        dir: "typescript",
        ext: "ts",
        header: "import { readFileSync } from \"fs\";\nimport type { Readable } from \"stream\";\n\n",
        item: ts_item,
    },
    Lang {
        dir: "go",
        ext: "go",
        header: "package fixture\n\nimport (\n\t\"errors\"\n\t\"strings\"\n)\n\n",
        item: go_item,
    },
];

fn rust_item(noun: &str, verb: &str, n: usize) -> String {
    let cap = capitalize(noun);
    format!(
        "/// {verb} the {noun} state and return the number of changed entries.
pub fn {verb}_{noun}_{n}(items: &mut HashMap<String, u64>, key: &str) -> Result<usize, String> {{
    // Bail out early so callers never see a half-updated map.
    if key.is_empty() {{
        return Err(format!(\"empty key for {noun}\"));
    }}
    let mut changed = 0;
    for (name, value) in items.iter_mut() {{
        if name.starts_with(key) && *value % {n} != 0 {{
            *value = value.wrapping_mul({n}).wrapping_add(1);
            changed += 1;
        }}
    }}
    Ok(changed)
}}

/// A {noun} record keyed by name.
#[derive(Debug, Clone)]
pub struct {cap}{n} {{
    pub name: String,
    pub weight: u64,
}}

impl fmt::Display for {cap}{n} {{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {{
        write!(f, \"{{}}:{{}}\", self.name, self.weight)
    }}
}}

"
    )
}

fn python_item(noun: &str, verb: &str, n: usize) -> String {
    let cap = capitalize(noun);
    let cap_verb = capitalize(verb);
    format!(
        "@dataclass
class {cap}{n}:
    \"\"\"A {noun} record keyed by name.\"\"\"

    name: str
    weight: int = 0


def {verb}_{noun}_{n}(items: dict, key: str) -> Optional[int]:
    \"\"\"{cap_verb} the {noun} state and return the number of changed entries.\"\"\"
    # Bail out early so callers never see a half-updated mapping.
    if not key:
        log.warning(\"empty key for {noun}\")
        return None
    changed = 0
    for name, value in list(items.items()):
        if name.startswith(key) and value % {n} != 0:
            items[name] = value * {n} + 1
            changed += 1
    return changed


"
    )
}

fn ts_item(noun: &str, verb: &str, n: usize) -> String {
    let cap = capitalize(noun);
    let cap_verb = capitalize(verb);
    format!(
        "/** A {noun} record keyed by name. */
export interface {cap}{n} {{
  name: string;
  weight: number;
}}

/** {cap_verb} the {noun} state and return the number of changed entries. */
export function {verb}{cap}{n}(items: Map<string, number>, key: string): number {{
  // Bail out early so callers never see a half-updated map.
  if (key.length === 0) {{
    throw new Error(`empty key for {noun}`);
  }}
  let changed = 0;
  for (const [name, value] of items) {{
    if (name.startsWith(key) && value % {n} !== 0) {{
      items.set(name, value * {n} + 1);
      changed += 1;
    }}
  }}
  return changed;
}}

"
    )
}

fn go_item(noun: &str, verb: &str, n: usize) -> String {
    let cap = capitalize(noun);
    let cap_verb = capitalize(verb);
    format!(
        "// {cap}{n} is a {noun} record keyed by name.
type {cap}{n} struct {{
\tName   string
\tWeight int
}}

// {cap_verb}{cap}{n} applies {verb} to the {noun} state and returns the number of changed entries.
func {cap_verb}{cap}{n}(items map[string]int, key string) (int, error) {{
\t// Bail out early so callers never see a half-updated map.
\tif key == \"\" {{
\t\treturn 0, errors.New(\"empty key for {noun}\")
\t}}
\tchanged := 0
\tfor name, value := range items {{
\t\tif strings.HasPrefix(name, key) && value%{n} != 0 {{
\t\t\titems[name] = value*{n} + 1
\t\t\tchanged++
\t\t}}
\t}}
\treturn changed, nil
}}

"
    )
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
        .unwrap_or_default()
}

fn render(lang: &Lang, rng: &mut Rng) -> String {
    let target = MIN_SIZE + rng.below(SIZE_SPAN);
    let mut out = lang.header.to_string();
    while out.len() < target {
        let (noun, verb) = (rng.pick(&NOUNS), rng.pick(&VERBS));
        out.push_str(&(lang.item)(noun, verb, rng.below(900) + 100));
    }
    out
}

pub fn generate(out: &Path) -> io::Result<()> {
    let mut rng = Rng(SEED);
    for lang in &LANGS {
        for i in 0..FILES_PER_LANG {
            let dir = out
                .join(lang.dir)
                .join(format!("pkg_{:02}", i / FILES_PER_DIR));
            fs::create_dir_all(&dir)?;
            fs::write(
                dir.join(format!("file_{i:03}.{}", lang.ext)),
                render(lang, &mut rng),
            )?;
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn main() -> io::Result<()> {
    let Some(out) = std::env::args_os().nth(1) else {
        eprintln!("usage: gen_fixture <out-dir>");
        std::process::exit(2);
    };
    generate(Path::new(&out))
}
