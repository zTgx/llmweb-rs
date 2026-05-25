use {
    crate::error::{LlmWebError, Result},
    headless_chrome::{Browser, LaunchOptions, LaunchOptionsBuilder, Tab},
    std::{ffi::OsStr, sync::Arc},
};

/// Thin wrapper around a `headless_chrome::Browser` configured with stealth flags.
///
/// The previous version of this struct held a single hidden tab and only let
/// callers fetch HTML by URL. To align with `llm-scraper`'s `Page`-based model,
/// the wrapper now exposes the underlying [`Browser`] and a helper to open a
/// new tab navigated to a URL. Higher-level code in `lib.rs` and `preprocess.rs`
/// works on a `Tab` so that callers (or library code) can drive the page
/// however they want before extraction.
pub struct LlmWebBrower {
    pub browser: Browser,
}

impl LlmWebBrower {
    pub async fn new() -> Result<LlmWebBrower> {
        Ok(Self {
            browser: stealthy_browser().await?,
        })
    }

    /// Open a new tab, navigate to `url`, and wait for navigation to settle.
    /// Returns the tab so the caller can run more interactions before handing
    /// it off to `preprocess` / `exec_on_tab` / `generate_on_tab` / etc.
    pub async fn open(&self, url: &str) -> Result<Arc<Tab>> {
        let tab = self
            .browser
            .new_tab()
            .map_err(|e| LlmWebError::Browser(format!("new_tab: {e}")))?;
        tab.navigate_to(url)
            .map_err(|e| LlmWebError::Browser(format!("navigate_to: {e}")))?;
        tab.wait_until_navigated()
            .map_err(|e| LlmWebError::Browser(format!("wait_until_navigated: {e}")))?;

        // Cheap upfront check for "JavaScript required" interstitials before
        // anyone tries to extract from a useless page.
        let html = tab
            .get_content()
            .map_err(|e| LlmWebError::Browser(format!("get_content: {e}")))?;
        if is_js_blocked(&html) {
            return Err(LlmWebError::JsBlocked);
        }

        Ok(tab)
    }
}

/// Evaluate a JS *expression* in the tab and decode the result as JSON.
///
/// The expression is wrapped in `JSON.stringify(...)` so that complex return
/// values come back as a string primitive (which the CDP serializer always
/// populates in `RemoteObject.value`, regardless of `returnByValue`).
///
/// Both sync and async IIFEs are supported — the wrapper awaits the inner
/// expression before stringifying.
pub fn evaluate_json(tab: &Arc<Tab>, expression: &str) -> Result<serde_json::Value> {
    // `await (EXPR)` is a no-op for sync values and a proper await for Promises.
    // Wrapping in an async IIFE lets `await_promise = true` resolve everything.
    let wrapped = format!(
        "(async () => {{ return JSON.stringify(await ({expression})); }})()"
    );
    let remote = tab
        .evaluate(&wrapped, true)
        .map_err(|e| LlmWebError::Browser(format!("evaluate: {e}")))?;

    let s = remote
        .value
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .ok_or_else(|| LlmWebError::Browser("evaluate returned no string value".into()))?;

    serde_json::from_str(&s).map_err(LlmWebError::SerdeJson)
}

pub fn is_js_blocked(html: &str) -> bool {
    html.contains("<h1>JavaScript is not available.</h1>") || html.contains("Please enable JavaScript")
}

async fn stealthy_browser() -> Result<Browser> {
    let opts = browser_launch_options().await?;
    Browser::new(opts).map_err(|e| LlmWebError::Browser(format!("Init Browser error: {e}")))
}

async fn browser_launch_options<'a>() -> Result<LaunchOptions<'a>> {
    let v: Vec<&OsStr> = vec![
        OsStr::new("--disable-blink-features=AutomationControlled"),
        OsStr::new("--no-sandbox"),
        OsStr::new("--disable-web-security"),
        OsStr::new(
            "--user-agent=Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/96.0.4664.45 Safari/537.36",
        ),
        OsStr::new("--lang=en-US,en;q=0.9"),
        OsStr::new("--disable-dev-shm-usage"),
        OsStr::new("--disable-gpu"),
        OsStr::new("--disable-infobars"),
        OsStr::new("--no-first-run"),
    ];

    LaunchOptionsBuilder::default()
        .headless(true)
        .window_size(Some((1200, 800)))
        .args(v)
        .build()
        .map_err(|e| LlmWebError::Browser(format!("{e}")))
}
