<div align="center">

# llmweb

**Extract any webpage to structured data — headless Chrome + LLM.**

[![Version](https://img.shields.io/crates/v/llmweb)](https://crates.io/crates/llmweb)
[![Downloads](https://img.shields.io/crates/d/llmweb?logo=rust)](https://crates.io/crates/llmweb)
[![License](https://img.shields.io/crates/l/llmweb)](LICENSE)
[![Documentation](https://img.shields.io/docsrs/llmweb)](https://docs.rs/llmweb)

</div>

## Install

```toml
[dependencies]
llmweb = "0.2"
```

Default config reads `OPENAI_API_KEY`. For any OpenAI-compatible gateway
(DeepSeek, Groq, OpenRouter, z.ai, vLLM, Ollama, ...) use `LlmWeb::with_client`:

```rust
use llmweb::{LlmWeb, openai::{Client, OpenAIConfig}};

let client = Client::with_config(
    OpenAIConfig::new()
        .with_api_base("https://api.deepseek.com/v1")
        .with_api_key("sk-..."),
);
let llmweb = LlmWeb::with_client(client, "deepseek-chat");
```

## Example

```rust
use llmweb::{Browser, Format, LlmWeb, RunOptions};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
struct SearchResult {
    title: String,
    url: Option<String>,
    snippet: Option<String>,
}

// json_schema mode requires an object root; wrap arrays in a named field.
#[derive(Debug, Serialize, Deserialize)]
struct GoogleSearchPage {
    query: String,
    results: Vec<SearchResult>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let browser = Browser::new().await?;
    let tab = browser
        .open("https://www.google.com/search?q=rust+programming+language")
        .await?;
    let llmweb = LlmWeb::new("gpt-4o-mini");

    let schema = json!({
        "type": "object",
        "properties": {
            "query":   { "type": "string" },
            "results": {
                "type": "array",
                "description": "Organic search results. Skip ads and knowledge cards.",
                "items": {
                    "type": "object",
                    "properties": {
                        "title":   { "type": "string" },
                        "url":     { "type": "string" },
                        "snippet": { "type": "string" }
                    },
                    "required": ["title"]
                }
            }
        },
        "required": ["query", "results"]
    });

    // Markdown gives the LLM a far cleaner view than Google's heavy HTML.
    let page: GoogleSearchPage = llmweb
        .exec_on_tab(
            &tab,
            schema,
            RunOptions { format: Format::Markdown, temperature: Some(0.0), ..Default::default() },
        )
        .await?;

    println!("Query: {}\n", page.query);
    for (i, r) in page.results.iter().enumerate() {
        println!("[{i}] {}", r.title);
        if let Some(u) = &r.url     { println!("    {u}"); }
        if let Some(s) = &r.snippet { println!("    {s}"); }
        println!();
    }
    Ok(())
}
```

URL-based shortcut (library opens the tab for you): `llmweb.exec(url, schema).await?`.

<details>
<summary>Sample output</summary>

```
Query: rust programming language

[0] Rust Programming Language
    https://www.rust-lang.org/en-US
    Rust's rich type system and ownership model guarantee memory-safety and thread-safety — enabling you to eliminate many classes of bugs at compile-time.

[1] Rust (programming language) - Wikipedia
    https://en.wikipedia.org/wiki/Rust_(programming_language)
    Rust is a general-purpose programming language which emphasizes performance, type safety, concurrency, and memory safety.

[2] Rust - A Living Hell - The Perspective From A Programmer Of 30 Years
    https://www.reddit.com/r/learnrust/comments/1binxlv/rust_a_living_hell_the_perspective_from_a/
    Mar 19, 2024 · This has been the worst experience learning a programming language that I have ever had by far. I found absolutely no joy in it in any shape or form.

[3] The Rust Programming Language - Reddit
    https://www.reddit.com/r/rust/
    r/rust: A place for all things related to the Rust programming language—an open-source systems language that emphasizes performance, reliability, and…

... (10 results total)
```

</details>

## Features

- **Extract** — `exec` / `exec_on_tab` / `exec_on_html`: one LLM call → typed `R`.
- **Stream** — `stream` / `stream_on_tab` / `stream_on_html`: incremental partial-JSON snapshots via [`partial_stream`](./src/streaming.rs).
- **Codegen** — `generate` → JS extractor string; `run_script` replays it with **zero LLM cost**.
- **Recipe** — `generate_recipe` → declarative CSS-selector recipe; `run_recipe` executes it in **pure Rust** (no eval, no LLM).
- **5 preprocessing modes** — `html` (in-browser DOM cleanup), `raw_html`, `markdown` (via `htmd`), `text`, `image` (base64 screenshot for vision models).
- **Multi-provider** — any OpenAI-compatible endpoint via `LlmWeb::with_client`.
- **Logging** — `tracing` integration; `RUST_LOG=llmweb=debug` shows LLM raw responses.

## Examples

| File | Demonstrates |
|---|---|
| [`hn.rs`](./examples/hn.rs)                   | Basic URL-shortcut extraction |
| [`google.rs`](./examples/google.rs)           | Browser + tab, real-world Google search |
| [`v2ex_stream.rs`](./examples/v2ex_stream.rs) | Streaming partial output |
| [`codegen.rs`](./examples/codegen.rs)         | Generate JS extractor, replay offline |
| [`recipe.rs`](./examples/recipe.rs)           | Generate CSS-selector recipe |
| [`inline_html.rs`](./examples/inline_html.rs) | No browser — extract from a `&str` |
| [`hn_custom.rs`](./examples/hn_custom.rs)     | Custom LLM endpoint + API key |

## CLI

```bash
cargo install llmweb
llmweb --schema-file schemas/hn_schema.json --format markdown <URL>
```

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=zTgx/llmweb&type=Date)](https://www.star-history.com/#zTgx/llmweb&Date)

## License

MIT
