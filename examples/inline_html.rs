//! Extract from a static HTML string — no browser, no network fetch.
//!
//! Useful when you already have HTML (fetched out-of-band via `reqwest`, a
//! feed, a test fixture, or pasted from a debug capture) and just want to run
//! LLM extraction against it.
//!
//! Env config (any OpenAI-compatible gateway):
//!
//!     export LLM_ENDPOINT="https://your-gateway/v1"
//!     export LLM_API_KEY="sk-..."
//!     export LLM_MODEL="your-model"
//!
//! Run:
//!     cargo run --example inline_html

use llmweb::{
    Format, LlmWeb, RunOptions,
    openai::{Client, OpenAIConfig},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
struct Product {
    name: String,
    price: f32,
    description: Option<String>,
}

/// OpenAI's strict `json_schema` mode requires the schema root to be an
/// object — arrays at the root are rejected. We wrap our list in `products`
/// here so the same code works against strict OpenAI and looser gateways alike.
#[derive(Debug, Serialize, Deserialize)]
struct Catalog {
    products: Vec<Product>,
}

const HTML: &str = r#"<!doctype html>
<html><body>
  <h1>Workshop Inventory</h1>
  <ul class="catalog">
    <li class="product" data-sku="KB-001">
      <h2>Mechanical Keyboard</h2>
      <span class="price">$129.99</span>
      <p>RGB backlit, hot-swappable switches, USB-C.</p>
    </li>
    <li class="product" data-sku="HUB-04">
      <h2>USB-C Hub</h2>
      <span class="price">$45.00</span>
      <p>7-in-1, 100W power delivery.</p>
    </li>
    <li class="product" data-sku="DSK-12">
      <h2>Standing Desk Mat</h2>
      <span class="price">$79.50</span>
      <p>Anti-fatigue, memory foam.</p>
    </li>
  </ul>
</body></html>"#;

#[tokio::main]
async fn main() {
    // Pipe library `tracing` output to stderr. Set RUST_LOG to control verbosity:
    //   RUST_LOG=llmweb=debug cargo run --example inline_html
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let endpoint = std::env::var("LLM_ENDPOINT")
        .unwrap_or_else(|_| "https://api.deepseek.com/v1".to_string());
    let api_key = std::env::var("LLM_API_KEY").expect("set LLM_API_KEY");
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string());

    let config = OpenAIConfig::new()
        .with_api_base(&endpoint)
        .with_api_key(&api_key);
    let client = Client::with_config(config);
    let llmweb = LlmWeb::with_client(client, &model);

    let schema = json!({
        "type": "object",
        "properties": {
            "products": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name":        { "type": "string" },
                        "price":       { "type": "number" },
                        "description": { "type": "string" }
                    },
                    "required": ["name", "price"]
                }
            }
        },
        "required": ["products"]
    });

    eprintln!("Extracting products from inline HTML via {endpoint} ({model})...");

    let catalog: Catalog = llmweb
        .exec_on_html(
            HTML,
            schema,
            RunOptions {
                format: Format::Markdown,
                temperature: Some(0.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    println!("--- LLM extraction ---");
    for p in &catalog.products {
        println!("  {} — ${:.2}", p.name, p.price);
        if let Some(d) = &p.description {
            println!("      {d}");
        }
    }
}
