//! Route A: ask the LLM to produce a JS extractor once, then replay it
//! against the live page with zero LLM calls.

use llmweb::LlmWeb;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Story {
    title: String,
    points: f32,
    by: Option<String>,
    comments_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HnPage {
    top: Vec<Story>,
}

#[tokio::main]
async fn main() {
    let schema_str = include_str!("../schemas/hn_schema.json");
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    let llmweb = LlmWeb::new("gemini-2.0-flash");

    eprintln!("[1/2] asking LLM to generate an extractor script...");
    let js = llmweb
        .generate("https://news.ycombinator.com", schema)
        .await
        .unwrap();
    println!("--- generated JS ---\n{js}\n--- end ---");

    eprintln!("[2/2] replaying the script without any LLM call...");
    let page: HnPage = llmweb
        .run_script("https://news.ycombinator.com", &js)
        .await
        .unwrap();
    println!("{:#?}", page.top);
}
