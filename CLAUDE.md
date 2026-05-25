# CLAUDE.md

Guidance for Claude Code when working with this repository.

## Project Overview

`llmweb` is a Rust crate that extracts structured data from arbitrary webpages by combining a headless Chrome browser with an LLM.

Ships as:
- A library crate (`llmweb`)
- A CLI binary (`llmweb`, `src/main.rs`)

The crate is published on crates.io (currently `0.1.7`) and is explicitly marked as under active development with unstable APIs.

## Architecture

```
URL ──► LlmWebBrower (headless_chrome) ──► Tab ──┐
                                                  │
                                          preprocess(tab, format)
                                                  │
                                                  ▼
                                          Preprocessed { content, format }
                                                  │
                  ┌───────────────────────────────┼──────────────────────────┐
                  ▼                               ▼                          ▼
        completion (JSON spec)         generate_extractor_js          generate_recipe_json
                  │                               │                          │
                  ▼                               ▼                          ▼
            JSON string                 JS source (IIFE)              JSON recipe
                  │                               │                          │
                  ▼                               ▼                          ▼
        serde_json::from_str       tab.evaluate (zero LLM)      scraper crate (zero LLM)
                  │                               │                          │
                  └───────────────────────────────┴──────────────────────────┘
                                                  │
                                                  ▼
                                            R: DeserializeOwned
```

### Modules

- `src/lib.rs` — public API. Exposes `LlmWeb` with three families of methods:
  1. **Inline extraction** — `exec` / `exec_with` / `stream` / `stream_with` (one-shot LLM call per page)
  2. **Route A — JS codegen** — `generate` / `generate_with` returns a JS extractor; `run_script` replays it with zero LLM calls
  3. **Route B — selector recipe** — `generate_recipe` / `generate_recipe_with` returns an [`ExtractRecipe`]; `run_recipe` executes it in pure Rust
- `src/preprocess.rs` — `Format` enum + `preprocess()`. Five modes:
  - `Html` (default) — in-browser cleanup (`CLEANUP_JS`), then `tab.get_content()`
  - `RawHtml` — unmodified `tab.get_content()`
  - `Markdown` — `document.body.innerHTML` → `htmd::convert`
  - `Text` — `document.body.innerText` via `tab.evaluate`
  - `Image` — PNG screenshot base64-encoded (sent as `ContentPart::Image` for multimodal models)
- `src/browser.rs` — stealthy `LlmWebBrower::new()` and `open(url) -> Arc<Tab>`. The free function `evaluate_json(tab, expr)` wraps the JS expression with `JSON.stringify(await (...))` so complex return values come back as a string primitive (CDP's `RemoteObject.value` is only populated for primitives when `returnByValue=false`, which is how the convenience `tab.evaluate` is configured).
- `src/models.rs` — `LLMClient` wrapping `genai::Client`. Three prompts:
  - `SYSTEM_PROMPT` — structured extraction (uses `JsonSpec`)
  - `CODEGEN_SYSTEM` — emits JS IIFE (no `JsonSpec` — output is source code)
  - `RECIPE_SYSTEM` — emits a recipe object (uses a meta-`JsonSpec`)
- `src/codegen.rs` — `run_script_on_tab()` for replaying generated JS against a live tab.
- `src/recipe.rs` — `ExtractRecipe` / `FieldRule` + pure-Rust executor on top of the `scraper` crate. Supports `attr` = `text` / `html` / any attribute name, and `parse` = `int` / `float` / `int_prefix`.
- `src/streaming.rs` — `repair_partial_json()` (close open brackets/strings, drop dangling keys, fill colons with `null`) + `partial_stream<R>()` that converts a `genai::ChatStream` into `Pin<Box<dyn Stream<Item = Result<R>>>>`.
- `src/error.rs` — `LlmWebError` enum (`Browser`, `ModelClient`, `SerdeJson`, `Io`, `JsBlocked`, `Preprocess`, `Recipe`).
- `src/main.rs` — `clap`-based CLI; `--format` maps to `Format`.

## Commands

```bash
# Build
cargo build
cargo build --release
cargo build --examples

# Examples (each needs the matching *_API_KEY env var)
cargo run --example hn
cargo run --example v2ex
cargo run --example v2ex_stream
cargo run --example codegen   # route A
cargo run --example recipe    # route B
cargo run --example x

# CLI
cargo run --bin llmweb -- \
  --schema-file schemas/hn_schema.json \
  --format markdown \
  https://news.ycombinator.com

# Tests, formatting, lint
cargo test
cargo fmt
cargo clippy -- -D warnings
```

`cargo test --lib` runs the offline unit tests (markdown stripping, recipe execution, partial-JSON repair). The integration test file `tests/llmweb.rs` is still an empty `// TODO:` placeholder.

## Working Notes

- The headless browser launches Chrome via `headless_chrome`; Chrome/Chromium must be on PATH. `--no-sandbox` + `--disable-web-security` are set unconditionally — keep this in mind when running in shared environments.
- `evaluate_json` wraps every JS expression in `(async () => JSON.stringify(await (EXPR)))()`. The CDP `Evaluate` call uses `returnByValue=false` (hardcoded inside `headless_chrome::Tab::evaluate`), so we always go through the string-primitive escape hatch.
- `run_recipe` deliberately uses `Format::RawHtml` for the underlying fetch, because the default `Html` mode strips attributes (`href`, `src`, etc.) that recipes typically depend on.
- The `Image` format sends a base64 screenshot via `genai::ContentPart::from_image_base64`. Providers without multimodal support will return an API error.
- `stream` does real incremental parsing: each chunk grows `buf`, `repair_partial_json` produces a syntactically-valid snapshot, `serde_json::from_str::<R>` is attempted, and successful new values are yielded. Snapshots that fail to parse are skipped silently — early ticks of an extraction with non-`Option` required fields won't emit until enough data has arrived.
- LLM-generated JS runs with full DOM privileges; if you scrape untrusted sites, prefer route B (recipe) which is pure Rust with no eval.
