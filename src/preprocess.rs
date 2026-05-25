//! Page preprocessing.
//!
//! Five format modes are supported. For a `custom` mode, call `evaluate_json`
//! yourself against the tab.
//!
//! - [`Format::Html`]      — run an in-browser cleanup pass, then take the
//!   serialized DOM (default).
//! - [`Format::RawHtml`]   — unmodified `tab.get_content()`.
//! - [`Format::Markdown`]  — `document.body.innerHTML` converted via `htmd`.
//! - [`Format::Text`]      — `document.body.innerText` (browser-native).
//! - [`Format::Image`]     — PNG screenshot encoded as base64 (for multimodal models).

use {
    crate::{
        browser::evaluate_json,
        error::{LlmWebError, Result},
    },
    headless_chrome::{Tab, protocol::cdp::Page::CaptureScreenshotFormatOption},
    std::sync::Arc,
};

/// Preprocessing format mode. See module docs for semantics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    Html,
    RawHtml,
    Markdown,
    Text,
    Image,
}

/// Options passed to high-level `LlmWeb` methods. All fields are optional;
/// use struct-update syntax to set only what you need:
///
/// ```ignore
/// RunOptions {
///     format: Format::Markdown,
///     temperature: Some(0.0),
///     ..Default::default()
/// }
/// ```
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub format: Format,
    /// Override the default system prompt. None = use the built-in prompt
    /// appropriate to the method (`SYSTEM_PROMPT` / `CODEGEN_SYSTEM` / `RECIPE_SYSTEM`).
    pub system: Option<String>,
    /// 0.0 = deterministic, higher = more random.
    pub temperature: Option<f64>,
    /// Nucleus-sampling threshold.
    pub top_p: Option<f64>,
    /// Hard cap on output tokens.
    pub max_tokens: Option<u32>,
}

impl RunOptions {
    /// Shorthand for `RunOptions { format, ..Default::default() }`.
    pub fn new(format: Format) -> Self {
        Self {
            format,
            ..Default::default()
        }
    }
}

/// Result of a preprocessing pass.
#[derive(Debug, Clone)]
pub struct Preprocessed {
    pub url: String,
    pub content: String,
    pub format: Format,
}

impl Preprocessed {
    /// MIME content type, only meaningful for [`Format::Image`].
    pub fn image_mime(&self) -> &'static str {
        "image/png"
    }
}

/// Strips heavy non-content tags and noisy attributes in the live DOM
/// before serialization.
const CLEANUP_JS: &str = r#"
(() => {
  const elementsToRemove = ['script','style','noscript','iframe','svg','img','audio','video','canvas','map','source','dialog','menu','menuitem','track','object','embed','form','input','button','select','textarea','label','option','optgroup','aside','footer','header','nav','head'];
  const attributesToRemove = ['style','src','alt','title','role','aria-','tabindex','on','data-'];
  document.querySelectorAll('*').forEach((el) => {
    if (elementsToRemove.includes(el.tagName.toLowerCase())) { el.remove(); return; }
    Array.from(el.attributes).forEach((attr) => {
      if (attributesToRemove.some((a) => attr.name.startsWith(a))) el.removeAttribute(attr.name);
    });
  });
  return true;
})()
"#;

/// Run the preprocessing pass on an already-opened tab.
pub async fn preprocess(tab: &Arc<Tab>, format: Format) -> Result<Preprocessed> {
    let url = tab.get_url();

    let content = match format {
        Format::Html => {
            // Mutate the live DOM, then serialize.
            let _ = evaluate_json(tab, CLEANUP_JS)
                .map_err(|e| LlmWebError::Preprocess(format!("cleanup: {e}")))?;
            tab.get_content()
                .map_err(|e| LlmWebError::Preprocess(format!("get_content: {e}")))?
        }
        Format::RawHtml => tab
            .get_content()
            .map_err(|e| LlmWebError::Preprocess(format!("get_content: {e}")))?,
        Format::Markdown => {
            let body_html = evaluate_json(tab, "document.body.innerHTML")
                .map_err(|e| LlmWebError::Preprocess(format!("body innerHTML: {e}")))?;
            let body_html = body_html
                .as_str()
                .ok_or_else(|| LlmWebError::Preprocess("innerHTML not a string".into()))?;
            htmd::convert(body_html)
                .map_err(|e| LlmWebError::Preprocess(format!("htmd: {e}")))?
        }
        Format::Text => {
            let txt = evaluate_json(tab, "document.body.innerText")
                .map_err(|e| LlmWebError::Preprocess(format!("body innerText: {e}")))?;
            txt.as_str()
                .ok_or_else(|| LlmWebError::Preprocess("innerText not a string".into()))?
                .to_string()
        }
        Format::Image => {
            let bytes = tab
                .capture_screenshot(CaptureScreenshotFormatOption::Png, None, None, true)
                .map_err(|e| LlmWebError::Preprocess(format!("screenshot: {e}")))?;
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(bytes)
        }
    };

    Ok(Preprocessed { url, content, format })
}
