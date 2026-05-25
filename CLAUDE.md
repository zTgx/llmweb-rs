# CLAUDE.md

Guidance for Claude Code when working with this repository.

## Project Overview

`llmweb` is a Rust crate that extracts structured data from arbitrary webpages by combining a headless Chrome browser with an LLM. It is the Rust counterpart of the TypeScript [`llm-scraper`](https://github.com/mishushakov/llm-scraper), and the public API is intentionally aligned with that library where feasible.

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
- `src/preprocess.rs` — `Format` enum + `preprocess()`. Five modes ported from `llm-scraper/src/preprocess.ts`:
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
- `src/error.rs` — `LlmWebError` enum (`Browser`, `ModelClient`, `SerdeJson`, `Io`, `JsBlocked`, `Preprocess`, `Recipe`).
- `src/main.rs` — `clap`-based CLI; `--format` maps to `Format`.

### Feature parity vs llm-scraper

| TS feature | Rust status |
|---|---|
| `html` / `raw_html` / `markdown` / `text` / `image` formats | ✅ |
| `custom` format function | ✗ (call `evaluate_json` yourself) |
| `scraper.run()` (one-shot LLM extraction) | ✅ `exec` / `exec_with` |
| `scraper.stream()` | ⚠️ token streaming only — final parse is still one-shot |
| `scraper.generate()` (code gen) | ✅ `generate` + `run_script` |
| Selector recipes | ✅ `generate_recipe` + `run_recipe` (Rust-specific, no TS equivalent) |
| Zod schemas | ✗ — JSON Schema (`serde_json::Value`) is the only option |
| Multimodal images | ✅ `Format::Image` → `ContentPart::Image` |

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

`cargo test --lib` runs the offline unit tests (markdown stripping + recipe execution). The integration test file `tests/llmweb.rs` is still an empty `// TODO:` placeholder.

## Working Notes

- The headless browser launches Chrome via `headless_chrome`; Chrome/Chromium must be on PATH. `--no-sandbox` + `--disable-web-security` are set unconditionally — keep this in mind when running in shared environments.
- `evaluate_json` wraps every JS expression in `(async () => JSON.stringify(await (EXPR)))()`. The CDP `Evaluate` call uses `returnByValue=false` (hardcoded inside `headless_chrome::Tab::evaluate`), so we always go through the string-primitive escape hatch.
- `run_recipe` deliberately uses `Format::RawHtml` for the underlying fetch, because the default `Html` mode strips attributes (`href`, `src`, etc.) that recipes typically depend on.
- The `Image` format sends a base64 screenshot via `genai::ContentPart::from_image_base64`. Providers without multimodal support will return an API error.
- The `stream` family currently buffers the full response before parsing — `genai` does emit tokens incrementally, but Rust-side partial JSON parsing is not yet wired up. Treat it as "console-streaming" only.
- LLM-generated JS runs with full DOM privileges; if you scrape untrusted sites, prefer route B (recipe) which is pure Rust with no eval.
