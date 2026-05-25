//! # llmweb
//!
//! Extract structured data from any webpage by combining a headless browser
//! with an LLM.
//!
//! - 5 preprocessing modes: `Html` (cleaned), `RawHtml`, `Markdown`, `Text`, `Image`.
//! - Code generation (route A): the LLM emits a JS extractor that runs in the
//!   browser via `tab.evaluate` — store it, replay it without further LLM cost.
//! - Selector recipe (route B): the LLM emits a declarative CSS-selector
//!   recipe, executed in pure Rust against any HTML.

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

pub use browser::LlmWebBrower as Browser;
pub use error::{LlmWebError, Result as LlmWebResult};
pub use headless_chrome::Tab;
pub use preprocess::{Format, Preprocessed, RunOptions};
pub use recipe::{ExtractRecipe, FieldRule};
pub use streaming::PartialStream;

/// Re-exports from `async-openai` so users can build a custom client without
/// taking a direct dependency on the crate version.
pub mod openai {
    pub use async_openai::{Client, config::OpenAIConfig};
}

/// The main client. Holds an `async-openai` client and the model name.
pub struct LlmWeb {
    client: models::LLMClient,
}

impl LlmWeb {
    /// Create a client using the default config — reads the API key from
    /// the `OPENAI_API_KEY` env var and hits the official OpenAI endpoint.
    /// For any other provider (DeepSeek, Groq, z.ai, OpenRouter, Ollama, ...)
    /// use [`Self::with_client`].
    pub fn new(name: &str) -> Self {
        Self {
            client: models::LLMClient::new(name),
        }
    }

    /// Create a client with a pre-built `async_openai::Client`. Use this to
    /// point at a custom endpoint, supply an inline API key, etc. Build the
    /// underlying client with `OpenAIConfig::new().with_api_base(...).with_api_key(...)`.
    pub fn with_client(
        client: ::async_openai::Client<::async_openai::config::OpenAIConfig>,
        model: &str,
    ) -> Self {
        Self {
            client: models::LLMClient::with_client(client, model),
        }
    }

    // ======================================================================
    // High-level: URL-based (library owns the browser).
    // Each one opens a stealthy browser, navigates, then delegates to its
    // `*_on_tab` counterpart.
    // ======================================================================

    pub async fn exec<R>(&self, url: &str, scheme: serde_json::Value) -> Result<R>
    where
        R: DeserializeOwned + Debug,
    {
        self.exec_with(url, scheme, RunOptions::default()).await
    }

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
        self.exec_on_tab(&tab, scheme, opts).await
    }

    pub async fn exec_from_schema_str<R>(&self, url: &str, schema_str: &str) -> Result<R>
    where
        R: DeserializeOwned + Debug,
    {
        let scheme: serde_json::Value = serde_json::from_str(schema_str)?;
        self.exec(url, scheme).await
    }

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
        self.stream_on_tab(&tab, scheme, opts).await
    }

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
        self.generate_on_tab(&tab, scheme, opts).await
    }

    pub async fn run_script<R>(&self, url: &str, js: &str) -> Result<R>
    where
        R: DeserializeOwned + Debug,
    {
        let browser = LlmWebBrower::new().await?;
        let tab = browser.open(url).await?;
        run_script_on_tab(&tab, js).await
    }

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
        self.generate_recipe_on_tab(&tab, scheme, opts).await
    }

    pub async fn run_recipe<R>(&self, url: &str, recipe: &ExtractRecipe) -> Result<R>
    where
        R: DeserializeOwned + Debug,
    {
        let browser = LlmWebBrower::new().await?;
        let tab = browser.open(url).await?;
        run_recipe_on_tab(&tab, recipe).await
    }

    // ======================================================================
    // Low-level: Tab-based (caller owns the browser).
    // Use these when you need to log in, click through, scroll-load, or
    // otherwise drive the page before extraction.
    // ======================================================================

    pub async fn exec_on_tab<R>(
        &self,
        tab: &Arc<Tab>,
        scheme: serde_json::Value,
        opts: RunOptions,
    ) -> Result<R>
    where
        R: DeserializeOwned + Debug,
    {
        let page = preprocess::preprocess(tab, opts.format).await?;
        let response = self.client.completion(&page, scheme, &opts).await?;
        Ok(serde_json::from_str(&response)?)
    }

    pub async fn stream_on_tab<R>(
        &self,
        tab: &Arc<Tab>,
        scheme: serde_json::Value,
        opts: RunOptions,
    ) -> Result<PartialStream<R>>
    where
        R: DeserializeOwned + Debug + Send + 'static + PartialEq,
    {
        let page = preprocess::preprocess(tab, opts.format).await?;
        let chat = self.client.completion_stream(&page, scheme, &opts).await?;
        Ok(streaming::partial_stream::<R>(chat))
    }

    pub async fn generate_on_tab(
        &self,
        tab: &Arc<Tab>,
        scheme: serde_json::Value,
        opts: RunOptions,
    ) -> Result<String> {
        guard_codegen_format(opts.format)?;
        let page = preprocess::preprocess(tab, opts.format).await?;
        self.client.generate_extractor_js(&page, &scheme, &opts).await
    }

    pub async fn generate_recipe_on_tab(
        &self,
        tab: &Arc<Tab>,
        scheme: serde_json::Value,
        opts: RunOptions,
    ) -> Result<ExtractRecipe> {
        guard_codegen_format(opts.format)?;
        let page = preprocess::preprocess(tab, opts.format).await?;
        let json = self.client.generate_recipe_json(&page, &scheme, &opts).await?;
        ExtractRecipe::from_json(&json)
    }

    // ======================================================================
    // No-browser: caller already has an HTML string.
    // Useful when you fetched HTML out-of-band (reqwest, a feed, a test fixture)
    // and just want LLM extraction without spinning up Chrome.
    //
    // `generate` (route A) isn't exposed here because the produced JS expects
    // a live DOM. Use `*_on_tab` for that path.
    // ======================================================================

    pub async fn exec_on_html<R>(
        &self,
        html: &str,
        scheme: serde_json::Value,
        opts: RunOptions,
    ) -> Result<R>
    where
        R: DeserializeOwned + Debug,
    {
        let page = preprocess::preprocess_html(html, opts.format)?;
        let response = self.client.completion(&page, scheme, &opts).await?;
        Ok(serde_json::from_str(&response)?)
    }

    pub async fn stream_on_html<R>(
        &self,
        html: &str,
        scheme: serde_json::Value,
        opts: RunOptions,
    ) -> Result<PartialStream<R>>
    where
        R: DeserializeOwned + Debug + Send + 'static + PartialEq,
    {
        let page = preprocess::preprocess_html(html, opts.format)?;
        let chat = self.client.completion_stream(&page, scheme, &opts).await?;
        Ok(streaming::partial_stream::<R>(chat))
    }

    pub async fn generate_recipe_on_html(
        &self,
        html: &str,
        scheme: serde_json::Value,
        opts: RunOptions,
    ) -> Result<ExtractRecipe> {
        guard_codegen_format(opts.format)?;
        let page = preprocess::preprocess_html(html, opts.format)?;
        let json = self.client.generate_recipe_json(&page, &scheme, &opts).await?;
        ExtractRecipe::from_json(&json)
    }
}

/// Run a previously-generated JS extractor against a tab the caller has
/// already navigated. No LLM call.
pub async fn run_script_on_tab<R>(tab: &Arc<Tab>, js: &str) -> Result<R>
where
    R: DeserializeOwned + Debug,
{
    codegen::run_script_on_tab(tab, js).await
}

/// Apply a recipe against the current state of a tab the caller has already
/// navigated. No LLM call. Uses raw HTML so attributes (`href`, `src`, etc.)
/// the recipe depends on are preserved.
pub async fn run_recipe_on_tab<R>(tab: &Arc<Tab>, recipe: &ExtractRecipe) -> Result<R>
where
    R: DeserializeOwned + Debug,
{
    let page = preprocess::preprocess(tab, Format::RawHtml).await?;
    let value = recipe.apply(&page.content)?;
    Ok(serde_json::from_value(value)?)
}

/// `generate*` only makes sense when the LLM can see the DOM structure — i.e.
/// HTML, not Markdown/Text/Image. Reject those at runtime.
fn guard_codegen_format(format: Format) -> Result<()> {
    match format {
        Format::Html | Format::RawHtml => Ok(()),
        other => Err(LlmWebError::Preprocess(format!(
            "code/recipe generation requires Format::Html or Format::RawHtml, got {other:?}"
        ))),
    }
}

