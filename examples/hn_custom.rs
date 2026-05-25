//! HN extraction with a custom endpoint + API key.
//!
//! Configure the LLM via env vars (works against any OpenAI-compatible
//! gateway — vLLM, OpenRouter, DeepSeek, Groq, a self-hosted proxy, etc.):
//!
//!     export LLM_ENDPOINT="https://api.deepseek.com/v1/"
//!     export LLM_API_KEY="sk-..."
//!     export LLM_MODEL="deepseek-chat"
//!
//! If your network can't reach news.ycombinator.com directly, point Chrome
//! through a proxy — the library picks up the standard env vars:
//!
//!     export HTTPS_PROXY="http://127.0.0.1:7890"
//!
//! Or just point at a reachable URL:
//!
//!     export LLM_URL="https://v2ex.com/go/vxna"
//!
//! Then:
//!     cargo run --example hn_custom

use llmweb::{
    LlmWeb, RunOptions,
    genai::{
        AdapterKind, AuthData, Client, Endpoint, ModelIden, ServiceTarget, ServiceTargetResolver,
    },
};
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
    let endpoint = std::env::var("LLM_ENDPOINT")
        .unwrap_or_else(|_| "https://api.deepseek.com/v1/".to_string());
    let api_key = std::env::var("LLM_API_KEY").expect("set LLM_API_KEY");
    let model =
        std::env::var("LLM_MODEL").unwrap_or_else(|_| "deepseek-chat".to_string());

    let endpoint_static: &'static str = Box::leak(endpoint.into_boxed_str());

    // Route every request through our custom endpoint + key, while keeping
    // the OpenAI wire format. Any OpenAI-compatible service works.
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

    let schema_str = include_str!("../schemas/hn_schema.json");
    let schema: serde_json::Value = serde_json::from_str(schema_str).unwrap();

    let url = std::env::var("LLM_URL")
        .unwrap_or_else(|_| "https://news.ycombinator.com".to_string());

    eprintln!("Extracting stories from {url} via {endpoint_static} ({model})...");

    let stories: Vec<Story> = llmweb
        .exec_with(
            &url,
            schema,
            RunOptions {
                temperature: Some(0.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    println!("{stories:#?}");
}
