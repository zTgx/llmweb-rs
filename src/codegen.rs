//! Route A — let the LLM generate a JavaScript extractor (IIFE) that runs in
//! the browser via `tab.evaluate`. The output is a plain `String` you can
//! persist to disk and replay without any LLM round-trip.
//!
//! Mirrors `llm-scraper`'s `scraper.generate()` + `page.evaluate(code)` flow.

use {
    crate::{
        browser::evaluate_json,
        error::{LlmWebError, Result},
    },
    headless_chrome::Tab,
    serde::de::DeserializeOwned,
    std::{fmt::Debug, sync::Arc},
};

/// Run a previously-generated JS extractor against a live tab and decode the
/// returned JSON into `R`. The script is expected to evaluate to a single
/// JSON-serializable value (typically via an IIFE).
pub async fn run_script_on_tab<R>(tab: &Arc<Tab>, js: &str) -> Result<R>
where
    R: DeserializeOwned + Debug,
{
    let value = evaluate_json(tab, js)?;
    serde_json::from_value(value).map_err(|e| {
        LlmWebError::ModelClient(format!("generated script returned unexpected shape: {e}"))
    })
}
