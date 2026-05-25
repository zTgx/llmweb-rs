<div align="center">

# llmweb

**Extract any webpage to structured data in Rust & LLM**

[![Version](https://img.shields.io/crates/v/llmweb)](https://crates.io/crates/llmweb)
[![Downloads](https://img.shields.io/crates/d/llmweb?logo=rust)](https://crates.io/crates/llmweb)
[![License](https://img.shields.io/crates/l/llmweb)](LICENSE)
[![Documentation](https://img.shields.io/docsrs/llmweb)](https://docs.rs/llmweb)

</div>

## Install

```toml
[dependencies]
llmweb     = "0.1"
```

Default config reads `OPENAI_API_KEY` from env. For any OpenAI-compatible
gateway (DeepSeek, Groq, z.ai, OpenRouter, vLLM, Ollama, ...) build a custom
client with `LlmWeb::with_client`:

```rust
use llmweb::{LlmWeb, openai::{Client, OpenAIConfig}};

let config = OpenAIConfig::new()
    .with_api_base("https://api.deepseek.com/v1")
    .with_api_key("sk-...");
let llmweb = LlmWeb::with_client(Client::with_config(config), "deepseek-chat");
```

## Example

```rust
use llmweb::{Browser, Format, LlmWeb, RunOptions};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
struct Story {
    title: String,
    points: f32,
    by: String,
    comments_url: String,
}

// OpenAI's json_schema mode requires an object root, so wrap the array.
#[derive(Debug, Serialize, Deserialize)]
struct HnPage {
    top: Vec<Story>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Launch a browser instance.
    let browser = Browser::new().await?;

    // 2. Open a new tab — drive it yourself if you need to log in, scroll, etc.
    let tab = browser.open("https://news.ycombinator.com").await?;

    // 3. Initialize the LLM client (uses OPENAI_API_KEY by default).
    let llmweb = LlmWeb::new("gpt-4o-mini");

    // 4. Define the schema (object-rooted; the array lives under `top`).
    let schema = json!({
        "type": "object",
        "properties": {
            "top": {
                "type": "array",
                "description": "Top 5 stories on Hacker News",
                "items": {
                    "type": "object",
                    "properties": {
                        "title":        { "type": "string" },
                        "points":       { "type": "number" },
                        "by":           { "type": "string" },
                        "comments_url": { "type": "string" }
                    },
                    "required": ["title", "points", "by", "comments_url"]
                }
            }
        },
        "required": ["top"]
    });

    // 5. Run the extraction.
    let page: HnPage = llmweb
        .exec_on_tab(&tab, schema, RunOptions { format: Format::Html, ..Default::default() })
        .await?;

    println!("{:#?}", page.top);
    Ok(())
}
```

For the URL-based shortcut that opens the tab internally, use `llmweb.exec(url, schema).await?`.

## CLI

- **CLI** — `cargo run --bin llmweb -- --schema-file schemas/hn_schema.json --format markdown <URL>`

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=zTgx/llmweb&type=Date)](https://www.star-history.com/#zTgx/llmweb&Date)

## License

MIT

