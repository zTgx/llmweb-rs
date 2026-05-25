use llmweb::LlmWeb;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Story {
    title: String,
    points: f32,
    by: Option<String>,
    comments_url: Option<String>,
}

/// Schema root is an object whose `top` field holds the array. This matches
/// OpenAI's strict `json_schema` mode (which rejects array-rooted schemas)
/// and aligns with the convention used by llm-scraper's HN example.
#[derive(Debug, Serialize, Deserialize)]
struct HnPage {
    top: Vec<Story>,
}

#[tokio::main]
async fn main() {
    let schema_str = include_str!("../schemas/hn_schema.json");

    let llmweb = LlmWeb::new("gemini-2.0-flash");
    eprintln!("Fetching from Hacker News and extracting stories...");

    let page: HnPage = llmweb
        .exec_from_schema_str("https://news.ycombinator.com", schema_str)
        .await
        .unwrap();
    println!("{:#?}", page.top);
}
