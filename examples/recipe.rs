//! Route B: ask the LLM to produce a declarative CSS-selector recipe once,
//! then execute it in pure Rust against the HTML — no LLM, no JS eval.

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
    let schema_str = include_str!("../schemas/hn_schema.json");
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    let llmweb = LlmWeb::new("gemini-2.0-flash");

    eprintln!("[1/2] asking LLM to generate a recipe...");
    let recipe = llmweb
        .generate_recipe("https://news.ycombinator.com", schema)
        .await
        .unwrap();
    println!("--- recipe ---\n{}\n--- end ---", serde_json::to_string_pretty(&recipe).unwrap());

    eprintln!("[2/2] applying recipe with zero LLM calls...");
    let stories: Vec<Story> = llmweb
        .run_recipe("https://news.ycombinator.com", &recipe)
        .await
        .unwrap();
    println!("{stories:#?}");
}
