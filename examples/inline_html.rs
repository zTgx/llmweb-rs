//! Extract from a static HTML string — no browser, no network fetch.
//!
//! Useful when you already have HTML (fetched out-of-band via `reqwest`, a
//! feed, a test fixture, or pasted from a debug capture) and just want to run
//! LLM extraction against it.
//!
//! Env config (same as the other custom examples):
//!
//!     export LLM_ENDPOINT="http://your-gateway/v1/"
//!     export LLM_API_KEY="sk-..."
//!     export LLM_MODEL="your-model"
//!
//! Run:
//!     cargo run --example inline_html

use llmweb::{
    Format, LlmWeb, RunOptions,
    genai::{
        AdapterKind, AuthData, Client, Endpoint, ModelIden, ServiceTarget, ServiceTargetResolver,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
struct Product {
    name: String,
    price: f32,
    description: Option<String>,
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
    // -- LLM config (same pattern as hn_custom / google_custom) --
    let endpoint = std::env::var("LLM_ENDPOINT")
        .unwrap_or_else(|_| "https://api.deepseek.com/v1/".to_string());
    let api_key = std::env::var("LLM_API_KEY").expect("set LLM_API_KEY");
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string());

    let endpoint_static: &'static str = Box::leak(endpoint.into_boxed_str());

    let resolver = ServiceTargetResolver::from_resolver_fn(
        move |t: ServiceTarget| -> Result<ServiceTarget, ::genai::resolver::Error> {
            Ok(ServiceTarget {
                endpoint: Endpoint::from_static(endpoint_static),
                auth: AuthData::from_single(api_key.clone()),
                model: ModelIden::new(AdapterKind::OpenAI, t.model.model_name),
            })
        },
    );

    let client = Client::builder()
        .with_service_target_resolver(resolver)
        .build();
    let llmweb = LlmWeb::with_client(client, &model);

    let schema = json!({
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
    });

    eprintln!("Extracting products from inline HTML via {endpoint_static} ({model})...");

    // No browser, no network fetch — the HTML is right there.
    let products: Vec<Product> = llmweb
        .exec_on_html(
            HTML,
            schema.clone(),
            RunOptions {
                // Markdown gives the LLM a cleaner view of structured content.
                format: Format::Markdown,
                temperature: Some(0.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    println!("--- LLM extraction ---");
    for p in &products {
        println!("  {} — ${:.2}", p.name, p.price);
        if let Some(d) = &p.description {
            println!("      {d}");
        }
    }

    // Same input, route B: ask the LLM for a CSS-selector recipe ONCE,
    // then replay it offline against the same HTML (or any other HTML
    // with the same structure) with zero further LLM calls.
    eprintln!("\nGenerating a reusable recipe...");
    let recipe = llmweb
        .generate_recipe_on_html(HTML, schema, RunOptions::default())
        .await
        .unwrap();
    println!("--- generated recipe ---");
    println!("{}", serde_json::to_string_pretty(&recipe).unwrap());

    eprintln!("\nReplaying recipe (no LLM call)...");
    let value = recipe.apply(HTML).unwrap();
    let replayed: Vec<Product> = serde_json::from_value(value).unwrap();
    println!("--- recipe replay ---");
    for p in &replayed {
        println!("  {} — ${:.2}", p.name, p.price);
    }
}
