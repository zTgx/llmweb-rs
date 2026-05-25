<div align="center">

# llmweb

**Extract any webpage to structured data in Rust & LLM**

[![Version](https://img.shields.io/crates/v/llmweb)](https://crates.io/crates/llmweb)
[![Downloads](https://img.shields.io/crates/d/llmweb?logo=rust)](https://crates.io/crates/llmweb)
[![License](https://img.shields.io/crates/l/llmweb)](LICENSE)
[![Documentation](https://img.shields.io/docsrs/llmweb)](https://docs.rs/llmweb)

</div>

## Features

- Schema-driven extraction (JSON Schema)
- Multi-provider LLMs via [`genai`] (OpenAI, Anthropic, Gemini, Groq, xAI, DeepSeek, Ollama, ...)
- 5 preprocessing modes: `html` (cleaned) / `raw_html` / `markdown` / `text` / `image`
- Code generation: ask the LLM once for a JS extractor, replay it later with zero LLM cost
- Selector recipes: pure-Rust execution via CSS selectors, no eval

## Install

```toml
[dependencies]
llmweb = "0.1"
```

Set the API key for your chosen provider:

```bash
export OPENAI_API_KEY=...
export ANTHROPIC_API_KEY=...
export GEMINI_API_KEY=...
# ... etc; Ollama needs no key
```

## Usage

```rust
use llmweb::LlmWeb;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Story {
    title: String,
    points: f32,
    by: Option<String>,
    comments_url: Option<String>,
}

#[tokio::main]
async fn main() {
    let schema = include_str!("../schemas/hn_schema.json");
    let stories: Vec<Story> = LlmWeb::new("gemini-2.0-flash")
        .exec_from_schema_str("https://news.ycombinator.com", schema)
        .await
        .unwrap();
    println!("{stories:#?}");
}
```

### Code generation (replay without LLM)

```rust
let llmweb = LlmWeb::new("gemini-2.0-flash");
let js = llmweb.generate(url, schema.clone()).await?;     // one LLM call
std::fs::write("extractor.js", &js)?;

let stories: Vec<Story> = llmweb.run_script(url, &js).await?; // zero LLM calls
```

### Selector recipe (pure Rust)

```rust
let recipe = llmweb.generate_recipe(url, schema.clone()).await?; // one LLM call
let stories: Vec<Story> = llmweb.run_recipe(url, &recipe).await?; // zero LLM calls
```

### CLI

```bash
cargo run --bin llmweb -- \
  --schema-file schemas/hn_schema.json \
  --format markdown \
  https://news.ycombinator.com
```

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=zTgx/llmweb&type=Date)](https://www.star-history.com/#zTgx/llmweb&Date)

## License

MIT — see [LICENSE](./LICENSE).

