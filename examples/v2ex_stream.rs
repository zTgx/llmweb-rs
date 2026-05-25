use futures::StreamExt;
use llmweb::LlmWeb;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[tokio::main]
async fn main() {
    let schema_str = include_str!("../schemas/v2ex_schema.json");
    let schema: Value = serde_json::from_str(schema_str).unwrap();

    // The stream yields progressively more-complete partial arrays.
    // Each tick prints how many items have arrived so far.
    let mut stream = LlmWeb::new("gemini-2.0-flash")
        .stream::<Vec<VXNA>>("https://v2ex.com/go/vxna", schema)
        .await
        .unwrap();

    let mut last_len = 0;
    while let Some(item) = stream.next().await {
        match item {
            Ok(partial) => {
                if partial.len() != last_len {
                    eprintln!("got {} items so far", partial.len());
                    last_len = partial.len();
                }
                // Final snapshot: print full.
                println!("{partial:#?}");
            }
            Err(e) => eprintln!("stream error: {e}"),
        }
    }
}
