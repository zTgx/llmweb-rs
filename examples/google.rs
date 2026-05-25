//! Real-world Google search extraction.
//!
//! This is the canonical "Browser + tab + LLM extraction" flow against a
//! page that's actually live on the internet.
//!
//! Env config:
//!     export LLM_ENDPOINT="https://your-gateway/v1"
//!     export LLM_API_KEY="sk-..."
//!     export LLM_MODEL="your-model"
//!     export GOOGLE_QUERY="rust async runtime"   # optional
//!
//! Run:
//!     cargo run --example google
//!     RUST_LOG=llmweb=debug cargo run --example google   # see LLM raw response
//!
//! Caveat: Google occasionally serves a CAPTCHA / "unusual traffic" page to
//! headless Chrome. If results come back empty or odd, that's almost certainly
//! the cause — try a different query, wait a bit, or run through a residential
//! proxy via `HTTPS_PROXY`.

use llmweb::{
    Browser, Format, LlmWeb, RunOptions,
    openai::{Client, OpenAIConfig},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
struct SearchResult {
    title: String,
    url: Option<String>,
    snippet: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GoogleSearchPage {
    query: String,
    results: Vec<SearchResult>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    // -- LLM config --
    let endpoint = std::env::var("LLM_ENDPOINT")
        .unwrap_or_else(|_| "https://api.deepseek.com/v1".to_string());
    let api_key = std::env::var("LLM_API_KEY").expect("set LLM_API_KEY");
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string());

    let client = Client::with_config(
        OpenAIConfig::new()
            .with_api_base(&endpoint)
            .with_api_key(&api_key),
    );
    let llmweb = LlmWeb::with_client(client, &model);

    // -- Target URL --
    let query =
        std::env::var("GOOGLE_QUERY").unwrap_or_else(|_| "rust programming language".to_string());
    let url = format!("https://www.google.com/search?q={}", url_encode(&query));

    // -- Browser --
    let browser = Browser::new().await?;
    let tab = browser.open(&url).await?;

    // -- Schema (object root; results array lives under `results`) --
    let schema = json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "The search query as it appears on the page" },
            "results": {
                "type": "array",
                "description": "Organic search results. Skip ads, knowledge cards, image carousels.",
                "items": {
                    "type": "object",
                    "properties": {
                        "title":   { "type": "string", "description": "Result headline" },
                        "url":     { "type": "string", "description": "Destination URL (the link target, not the green breadcrumb)" },
                        "snippet": { "type": "string", "description": "Description text under the title" }
                    },
                    "required": ["title"]
                }
            }
        },
        "required": ["query", "results"]
    });

    eprintln!("Searching '{query}' on Google via {endpoint} ({model})...");

    let page: GoogleSearchPage = llmweb
        .exec_on_tab(
            &tab,
            schema,
            RunOptions {
                // Markdown gives the LLM a vastly cleaner view than Google's
                // raw HTML (which is huge, JS-heavy, and full of noise).
                format: Format::Markdown,
                temperature: Some(0.0),
                ..Default::default()
            },
        )
        .await?;

    println!("\nQuery: {}\n", page.query);
    for (i, r) in page.results.iter().enumerate() {
        println!("[{i}] {}", r.title);
        if let Some(u) = &r.url {
            println!("    {u}");
        }
        if let Some(s) = &r.snippet {
            println!("    {s}");
        }
        println!();
    }

    Ok(())
}

/// Minimal URL-component encoder so we don't pull in `urlencoding` for one call.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
