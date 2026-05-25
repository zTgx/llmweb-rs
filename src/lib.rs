//! # llmweb
//!
//! Extract structured data from any webpage by combining a headless browser
//! with an LLM. Aligned with the TypeScript [`llm-scraper`] library:
//!
//! - 5 preprocessing modes: `Html` (cleaned), `RawHtml`, `Markdown`, `Text`, `Image`.
//! - Code generation (route A): the LLM emits a JS extractor that runs in the
//!   browser via `tab.evaluate` — store it, replay it without further LLM cost.
//! - Selector recipe (route B): the LLM emits a declarative CSS-selector
//!   recipe, executed in pure Rust against any HTML.
//!
//! [`llm-scraper`]: https://github.com/mishushakov/llm-scraper

use {
    crate::{browser::LlmWebBrower, error::Result},
    serde::de::DeserializeOwned,
    std::{fmt::Debug, sync::Arc},
};

mod browser;
mod codegen;
pub mod error;
mod models;
pub mod preprocess;
pub mod recipe;
pub mod streaming;

pub use preprocess::{Format, Preprocessed, RunOptions};
pub use recipe::{ExtractRecipe, FieldRule};
pub use streaming::PartialStream;

/// The main client.
pub struct LlmWeb {
    client: models::LLMClient,
}

impl LlmWeb {
    /// Create a new client for a given model name (e.g. `"gemini-2.0-flash"`,
    /// `"gpt-4o"`, `"claude-3-5-sonnet"`). Provider routing is delegated to `genai`.
    pub fn new(name: &str) -> Self {
        Self {
            client: models::LLMClient::new(name),
        }
    }

    // ------------------------------------------------------------------
    // High-level: URL-based, library owns the browser.
    // ------------------------------------------------------------------

    /// One-shot extraction with default preprocessing (cleaned HTML).
    pub async fn exec<R>(&self, url: &str, scheme: serde_json::Value) -> Result<R>
    where
        R: DeserializeOwned + Debug,
    {
        self.exec_with(url, scheme, RunOptions::default()).await
    }

    /// One-shot extraction with explicit options (e.g. format).
    pub async fn exec_with<R>(
        &self,
        url: &str,
        scheme: serde_json::Value,
        opts: RunOptions,
    ) -> Result<R>
    where
        R: DeserializeOwned + Debug,
    {
        let browser = LlmWebBrower::new().await?;
        let tab = browser.open(url).await?;
        let page = preprocess::preprocess(&tab, opts.format).await?;
        let response = self.client.completion(&page, scheme).await?;
        Ok(serde_json::from_str(&response)?)
    }

    /// Convenience wrapper that parses a schema string for you.
    pub async fn exec_from_schema_str<R>(&self, url: &str, schema_str: &str) -> Result<R>
    where
        R: DeserializeOwned + Debug,
    {
        let scheme: serde_json::Value = serde_json::from_str(schema_str)?;
        self.exec(url, scheme).await
    }

    /// Real incremental streaming. Returns a `Stream<Item = Result<R>>` that
    /// yields progressively more-complete partial values as the LLM emits
    /// tokens. Duplicate consecutive snapshots are filtered.
    ///
    /// For `R` with non-`Option` fields, early ticks will fail to parse and
    /// be skipped — you'll only see emissions once enough required fields
    /// have streamed in. Use `serde_json::Value` or an all-`Option` struct
    /// to receive every tick.
    pub async fn stream<R>(
        &self,
        url: &str,
        scheme: serde_json::Value,
    ) -> Result<PartialStream<R>>
    where
        R: DeserializeOwned + Debug + Send + 'static + PartialEq,
    {
        self.stream_with(url, scheme, RunOptions::default()).await
    }

    pub async fn stream_with<R>(
        &self,
        url: &str,
        scheme: serde_json::Value,
        opts: RunOptions,
    ) -> Result<PartialStream<R>>
    where
        R: DeserializeOwned + Debug + Send + 'static + PartialEq,
    {
        let browser = LlmWebBrower::new().await?;
        let tab = browser.open(url).await?;
        let page = preprocess::preprocess(&tab, opts.format).await?;
        let chat = self.client.completion_stream(&page, scheme).await?;
        Ok(streaming::partial_stream::<R>(chat))
    }

    // ------------------------------------------------------------------
    // Route A — JS code generation.
    // ------------------------------------------------------------------

    /// Ask the LLM to produce a JS IIFE that extracts data matching `scheme`
    /// from the page. The returned string can be persisted and replayed via
    /// [`LlmWeb::run_script`] with no further LLM call.
    pub async fn generate(&self, url: &str, scheme: serde_json::Value) -> Result<String> {
        self.generate_with(url, scheme, RunOptions::default()).await
    }

    pub async fn generate_with(
        &self,
        url: &str,
        scheme: serde_json::Value,
        opts: RunOptions,
    ) -> Result<String> {
        let browser = LlmWebBrower::new().await?;
        let tab = browser.open(url).await?;
        let page = preprocess::preprocess(&tab, opts.format).await?;
        self.client.generate_extractor_js(&page, &scheme).await
    }

    /// Execute a previously-generated JS extractor against `url`. No LLM call.
    pub async fn run_script<R>(&self, url: &str, js: &str) -> Result<R>
    where
        R: DeserializeOwned + Debug,
    {
        let browser = LlmWebBrower::new().await?;
        let tab = browser.open(url).await?;
        run_script_on_tab(&tab, js).await
    }

    // ------------------------------------------------------------------
    // Route B — selector recipe.
    // ------------------------------------------------------------------

    /// Ask the LLM to produce a declarative selector recipe (route B). Cheaper
    /// to store and replay than [`generate`], at the cost of expressiveness.
    pub async fn generate_recipe(
        &self,
        url: &str,
        scheme: serde_json::Value,
    ) -> Result<ExtractRecipe> {
        self.generate_recipe_with(url, scheme, RunOptions::default()).await
    }

    pub async fn generate_recipe_with(
        &self,
        url: &str,
        scheme: serde_json::Value,
        opts: RunOptions,
    ) -> Result<ExtractRecipe> {
        let browser = LlmWebBrower::new().await?;
        let tab = browser.open(url).await?;
        let page = preprocess::preprocess(&tab, opts.format).await?;
        let json = self.client.generate_recipe_json(&page, &scheme).await?;
        ExtractRecipe::from_json(&json)
    }

    /// Execute a recipe against a fresh fetch of `url`. The recipe runs in
    /// pure Rust (no browser navigation past the initial load, no LLM).
    pub async fn run_recipe<R>(&self, url: &str, recipe: &ExtractRecipe) -> Result<R>
    where
        R: DeserializeOwned + Debug,
    {
        let browser = LlmWebBrower::new().await?;
        let tab = browser.open(url).await?;
        // Use raw HTML for recipe matching — cleanup would remove attributes
        // (href, src, etc.) the recipe depends on.
        let page = preprocess::preprocess(&tab, Format::RawHtml).await?;
        let value = recipe.apply(&page.content)?;
        Ok(serde_json::from_value(value)?)
    }
}

/// Re-export of the low-level helper for advanced users who manage their own
/// browser/tab lifecycle (e.g. to log in, scroll, or interact before extraction).
pub async fn run_script_on_tab<R>(
    tab: &Arc<headless_chrome::Tab>,
    js: &str,
) -> Result<R>
where
    R: DeserializeOwned + Debug,
{
    codegen::run_script_on_tab(tab, js).await
}
