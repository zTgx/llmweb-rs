use llmweb::LlmWeb;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VXNA {
    pub username: String,
    pub avatar_url: String,
    pub profile_url: String,
    pub title: String,
    pub topic_url: String,
    pub topic_id: u64,
    pub relative_time: String,
    pub reply_count: u32,
    pub last_replier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct V2exPage {
    items: Vec<VXNA>,
}

#[tokio::main]
async fn main() {
    let schema_str = include_str!("../schemas/v2ex_schema.json");

    let llmweb = LlmWeb::new("gemini-2.0-flash");
    let page: V2exPage = llmweb
        .exec_from_schema_str("https://v2ex.com/go/vxna", schema_str)
        .await
        .unwrap();
    println!("{:#?}", page.items);
}
