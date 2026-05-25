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

### Custom model / endpoint / API key

Build a `genai::Client` with a `ServiceTargetResolver` and pass it in:

```rust
use llmweb::{LlmWeb, genai::{
    AdapterKind, AuthData, Client, Endpoint, ModelIden, ServiceTarget, ServiceTargetResolver,
}};

let resolver = ServiceTargetResolver::from_resolver_fn(
    |t: ServiceTarget| -> Result<ServiceTarget, ::genai::resolver::Error> {
        Ok(ServiceTarget {
            endpoint: Endpoint::from_static("https://api.my-llm.com/v1/"),
            auth:     AuthData::from_single("sk-my-key"),
            model:    ModelIden::new(AdapterKind::OpenAI, t.model.model_name),
        })
    },
);
let client = Client::builder().with_service_target_resolver(resolver).build();
let llmweb = LlmWeb::with_client(client, "my-model-name");
```

`AdapterKind` chooses the wire protocol (`OpenAI`, `Anthropic`, `Gemini`, ...). Any OpenAI-compatible gateway works with `AdapterKind::OpenAI`.

### Page-based usage (login, scroll, etc.)

For sites that need interaction before extraction, drive the browser yourself and call the `*_on_tab` variants:

```rust
use llmweb::{Browser, LlmWeb, RunOptions};

let browser = Browser::new().await?;
let tab = browser.open("https://example.com/login").await?;
tab.find_element("input[name=email]")?.click()?.type_into("me@example.com")?;
tab.find_element("input[name=password]")?.click()?.type_into("...")?;
tab.find_element("button[type=submit]")?.click()?;
tab.wait_until_navigated()?;

let stories: Vec<Story> = LlmWeb::new("gemini-2.0-flash")
    .exec_on_tab(&tab, schema, RunOptions::default())
    .await?;
```

### Tuning the LLM call

`RunOptions` exposes the common knobs:

```rust
let opts = llmweb::RunOptions {
    format: llmweb::Format::Markdown,
    temperature: Some(0.0),
    max_tokens: Some(2048),
    system: Some("You are a careful extractor. Prefer null over guesses.".into()),
    ..Default::default()
};
let result: Vec<Story> = llmweb.exec_with(url, schema, opts).await?;
```

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=zTgx/llmweb&type=Date)](https://www.star-history.com/#zTgx/llmweb&Date)

## License

MIT — see [LICENSE](./LICENSE).

