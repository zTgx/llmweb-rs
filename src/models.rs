use {
    crate::{
        error::{LlmWebError, Result},
        preprocess::{Format, Preprocessed},
    },
    genai::{
        Client,
        chat::{
            ChatMessage, ChatOptions, ChatRequest, ChatResponseFormat, ChatStream, ContentPart,
            JsonSpec, MessageContent,
        },
    },
    serde_json::{Value, json},
};

pub const SYSTEM_PROMPT: &str = "You are a structured information extraction assistant. Please extract JSON from the HTML page.\nStrictly output the JSON structure as specified above. Use null for missing fields.";

pub const CODEGEN_SYSTEM: &str = "You are a web scraping code generator.\n\nGiven a JSON Schema and the current page's DOM, write a SINGLE JavaScript expression that, when evaluated against the live DOM, returns a value matching the schema.\n\nSTRICT REQUIREMENTS:\n- Output ONLY the JavaScript expression. No prose, no markdown fences, no imports, no console.log.\n- The expression MUST be an IIFE — either `(() => { ... })()` or `(async () => { ... })()`.\n- The returned value MUST be JSON-serializable: no DOM nodes, no functions, no Symbols.\n- Use document.querySelector / querySelectorAll / element.textContent / element.getAttribute / element.innerText.\n- The returned value's shape MUST match the provided schema exactly.";

pub const RECIPE_SYSTEM: &str = "You are a web-scraping recipe generator.\n\nGiven a target JSON Schema and an HTML page, output a JSON RECIPE that describes how to extract the schema using CSS selectors. Do NOT extract the actual data — only the rules.\n\nRecipe format:\n{\n  \"container\": \"<optional CSS selector matching each item; omit/null for a single-object schema>\",\n  \"fields\": {\n    \"<field_name>\": {\n      \"selector\": \"<CSS selector, evaluated inside container if container is set>\",\n      \"attr\": \"text\" | \"html\" | \"<attribute name like href, src>\",\n      \"parse\": null | \"int\" | \"float\" | \"int_prefix\"\n    }\n  }\n}\n\nRules:\n- For array-of-object schemas: set `container` to the selector matching one item; each field selector is relative to that item.\n- For object schemas (no array): omit `container`; each field selector is evaluated against the whole document.\n- `attr` defaults to \"text\" (textContent). Use \"html\" for innerHTML, or an attribute name like \"href\" for links.\n- `parse: \"int_prefix\"` extracts the leading integer from text (e.g. \"42 points\" -> 42). Use for numeric fields whose text has units.\n- Output ONLY the JSON object. No prose, no markdown fences.";

#[macro_export]
macro_rules! strip_markdown_backticks {
    ($text:expr) => {{
        let trimmed = $text.trim();
        let re_leading = regex::Regex::new(r"(?i)^```[\w]*\s*").unwrap();
        let re_trailing = regex::Regex::new(r"(?i)\s*```$").unwrap();
        let without_leading = re_leading.replace(trimmed, "");
        let without_trailing = re_trailing.replace(&without_leading, "");
        without_trailing.to_string()
    }};
}

pub struct LLMClient {
    client: Client,
    pub model: String,
}

impl LLMClient {
    pub fn new(model: &str) -> Self {
        Self {
            client: Client::default(),
            model: model.to_string(),
        }
    }

    /// One-shot JSON extraction. Accepts a `Preprocessed` so the caller can
    /// choose how the page was rendered (html / markdown / image / ...).
    pub async fn completion(&self, page: &Preprocessed, scheme: Value) -> Result<String> {
        let op = ChatOptions::default().with_response_format(ChatResponseFormat::JsonSpec(
            JsonSpec::new("LlmWeb", json!(scheme)),
        ));

        let chat_req = ChatRequest::new(vec![
            ChatMessage::system(SYSTEM_PROMPT),
            user_message_for_page(page),
        ]);

        let response = self
            .client
            .exec_chat(&self.model, chat_req, Some(&op))
            .await
            .map_err(|e| LlmWebError::ModelClient(format!("{e}")))?;

        extract_text(response)
    }

    /// Open a streaming chat that returns text chunks. The caller is
    /// responsible for accumulating + (partially) parsing the chunks.
    /// See `crate::streaming` for the standard partial-JSON consumer.
    pub async fn completion_stream(&self, page: &Preprocessed, scheme: Value) -> Result<ChatStream> {
        let op = ChatOptions::default().with_response_format(ChatResponseFormat::JsonSpec(
            JsonSpec::new("LlmWeb", json!(scheme)),
        ));

        let chat_req = ChatRequest::new(vec![
            ChatMessage::system(SYSTEM_PROMPT),
            user_message_for_page(page),
        ]);

        let response = self
            .client
            .exec_chat_stream(&self.model, chat_req, Some(&op))
            .await
            .map_err(|e| LlmWebError::ModelClient(format!("{e}")))?;

        Ok(response.stream)
    }

    /// Generate a JS IIFE that extracts data matching `scheme` from the page.
    /// Returned string is raw JavaScript, ready to feed into `tab.evaluate`.
    pub async fn generate_extractor_js(&self, page: &Preprocessed, scheme: &Value) -> Result<String> {
        let user_text = format!(
            "Target schema:\n{}\n\nPage URL: {}\nPage content (for reference; your code will run against the LIVE DOM, not this snapshot):\n{}",
            serde_json::to_string_pretty(scheme)?,
            page.url,
            page.content,
        );

        let chat_req = ChatRequest::new(vec![
            ChatMessage::system(CODEGEN_SYSTEM),
            ChatMessage::user(user_text),
        ]);

        // No JsonSpec — we want JS source, not JSON.
        let response = self
            .client
            .exec_chat(&self.model, chat_req, None)
            .await
            .map_err(|e| LlmWebError::ModelClient(format!("{e}")))?;

        let text = extract_text(response)?;
        Ok(strip_markdown_backticks!(text))
    }

    /// Generate a selector recipe (route B). Output is a JSON object describing
    /// CSS selectors per field; see [`crate::recipe::ExtractRecipe`].
    pub async fn generate_recipe_json(&self, page: &Preprocessed, scheme: &Value) -> Result<String> {
        let recipe_meta_schema = json!({
            "type": "object",
            "properties": {
                "container": { "type": ["string", "null"] },
                "fields": {
                    "type": "object",
                    "additionalProperties": {
                        "type": "object",
                        "properties": {
                            "selector": { "type": "string" },
                            "attr":     { "type": "string" },
                            "parse":    { "type": ["string", "null"] }
                        },
                        "required": ["selector"]
                    }
                }
            },
            "required": ["fields"]
        });

        let op = ChatOptions::default().with_response_format(ChatResponseFormat::JsonSpec(
            JsonSpec::new("LlmWebRecipe", recipe_meta_schema),
        ));

        let user_text = format!(
            "Target schema:\n{}\n\nPage URL: {}\nPage content:\n{}",
            serde_json::to_string_pretty(scheme)?,
            page.url,
            page.content,
        );

        let chat_req = ChatRequest::new(vec![
            ChatMessage::system(RECIPE_SYSTEM),
            ChatMessage::user(user_text),
        ]);

        let response = self
            .client
            .exec_chat(&self.model, chat_req, Some(&op))
            .await
            .map_err(|e| LlmWebError::ModelClient(format!("{e}")))?;

        let text = extract_text(response)?;
        Ok(strip_markdown_backticks!(text))
    }
}

/// Build a user message from a `Preprocessed`. For image format the content is
/// sent as a base64 image part; everything else is plain text.
fn user_message_for_page(page: &Preprocessed) -> ChatMessage {
    if page.format == Format::Image {
        let parts: Vec<ContentPart> = vec![
            ContentPart::from_text("Extract structured data from the screenshot of the page below."),
            ContentPart::from_image_base64(page.image_mime(), page.content.clone()),
        ];
        ChatMessage::user(MessageContent::from_parts(parts))
    } else {
        ChatMessage::user(page.content.clone())
    }
}

fn extract_text(response: genai::chat::ChatResponse) -> Result<String> {
    let json_str = response
        .content
        .ok_or_else(|| LlmWebError::ModelClient("No content in response".to_string()))?
        .text_into_string();

    if let Some(json_str) = json_str {
        return Ok(strip_markdown_backticks!(json_str));
    }
    Err(LlmWebError::ModelClient("Content to string error".to_string()))
}

#[cfg(test)]
mod tests {
    use regex;

    #[test]
    fn test_strip_markdown_backticks() {
        let s1 = "hello";
        assert_eq!(strip_markdown_backticks!(s1), "hello");

        let s2 = "```json\n{\"a\":1}\n```";
        assert_eq!(strip_markdown_backticks!(s2), "{\"a\":1}");

        let s3 = "```rust\nlet x = 1;\n```";
        assert_eq!(strip_markdown_backticks!(s3), "let x = 1;");

        let s4 = "   ```json\n{\"b\":2}\n```   ";
        assert_eq!(strip_markdown_backticks!(s4), "{\"b\":2}");

        let s5 = "```";
        assert_eq!(strip_markdown_backticks!(s5), "");

        let s6 = "some `inline` code";
        assert_eq!(strip_markdown_backticks!(s6), "some `inline` code");
    }
}
