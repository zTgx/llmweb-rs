use thiserror::Error;

/// The main error type for the `llmweb` crate.
#[derive(Error, Debug)]
pub enum LlmWebError {
    /// An error originating from the headless browser interaction.
    #[error("Browser error: {0}")]
    Browser(String),

    /// An error from the LLM client (e.g., API errors).
    #[error("Model client error: {0}")]
    ModelClient(String),

    /// An error during JSON serialization or deserialization.
    #[error("Serde JSON error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    /// An I/O error, typically from reading a file.
    #[error("I/O error: {0}")]
    Io(String),

    /// An JsBlocked error.
    #[error("JsBlocked error")]
    JsBlocked,

    /// An error during HTML preprocessing (markdown conversion, etc.).
    #[error("Preprocess error: {0}")]
    Preprocess(String),

    /// An error while applying an extraction recipe (invalid CSS selector,
    /// missing required field, parse failure).
    #[error("Recipe error: {0}")]
    Recipe(String),
}

/// A specialized `Result` type for `llmweb` operations.
pub type Result<T> = std::result::Result<T, LlmWebError>;
