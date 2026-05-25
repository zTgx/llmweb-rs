use {
    crate::{
        error::{LlmWebError, Result},
        preprocess::{Format, Preprocessed, RunOptions},
    },
    async_openai::{
        Client,
        config::OpenAIConfig,
        types::chat::{
            ChatCompletionRequestMessageContentPartImage,
            ChatCompletionRequestMessageContentPartText, ChatCompletionRequestSystemMessageArgs,
            ChatCompletionRequestUserMessage, ChatCompletionRequestUserMessageContent,
            ChatCompletionRequestUserMessageContentPart, ChatCompletionResponseStream,
            CreateChatCompletionRequest, CreateChatCompletionRequestArgs, ImageUrl,
            ResponseFormat, ResponseFormatJsonSchema,
        },
    },
    serde_json::Value,
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
    client: Client<OpenAIConfig>,
    pub model: String,
}

impl LLMClient {
    pub fn new(model: &str) -> Self {
        Self {
            client: Client::with_config(OpenAIConfig::new()),
            model: model.to_string(),
        }
    }

    pub fn with_client(client: Client<OpenAIConfig>, model: &str) -> Self {
        Self {
            client,
            model: model.to_string(),
        }
    }

    /// One-shot JSON extraction.
    pub async fn completion(
        &self,
        page: &Preprocessed,
        scheme: Value,
        opts: &RunOptions,
    ) -> Result<String> {
        let request = build_request(
            &self.model,
            page,
            opts,
            SYSTEM_PROMPT,
            Some(&scheme),
            Some(ResponseFormat::JsonSchema {
                json_schema: ResponseFormatJsonSchema {
                    name: "LlmWeb".to_string(),
                    description: None,
                    schema: scheme.clone(),
                    strict: None,
                },
            }),
            false,
        )?;

        let response = self
            .client
            .chat()
            .create(request)
            .await
            .map_err(|e| LlmWebError::ModelClient(format!("{e}")))?;

        let text = response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| LlmWebError::ModelClient("no content in response".into()))?;

        tracing::debug!(target: "llmweb::completion", raw = %text, "LLM raw response");
        Ok(strip_markdown_backticks!(text))
    }

    /// Open a streaming chat. Caller drives the SSE stream and parses chunks.
    pub async fn completion_stream(
        &self,
        page: &Preprocessed,
        scheme: Value,
        opts: &RunOptions,
    ) -> Result<ChatCompletionResponseStream> {
        let request = build_request(
            &self.model,
            page,
            opts,
            SYSTEM_PROMPT,
            Some(&scheme),
            Some(ResponseFormat::JsonSchema {
                json_schema: ResponseFormatJsonSchema {
                    name: "LlmWeb".to_string(),
                    description: None,
                    schema: scheme.clone(),
                    strict: None,
                },
            }),
            true,
        )?;

        self.client
            .chat()
            .create_stream(request)
            .await
            .map_err(|e| LlmWebError::ModelClient(format!("{e}")))
    }

    /// Generate a JS IIFE that extracts data matching `scheme` from the page.
    pub async fn generate_extractor_js(
        &self,
        page: &Preprocessed,
        scheme: &Value,
        opts: &RunOptions,
    ) -> Result<String> {
        let user_text = format!(
            "Target schema:\n{}\n\nPage URL: {}\nPage content (for reference; your code will run against the LIVE DOM, not this snapshot):\n{}",
            serde_json::to_string_pretty(scheme)?,
            page.url,
            page.content,
        );

        let request = build_text_request(
            &self.model,
            opts,
            CODEGEN_SYSTEM,
            user_text,
            None, // no JsonSpec — output is JS source, not JSON
        )?;

        let response = self
            .client
            .chat()
            .create(request)
            .await
            .map_err(|e| LlmWebError::ModelClient(format!("{e}")))?;

        let text = response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| LlmWebError::ModelClient("no content in response".into()))?;

        Ok(strip_markdown_backticks!(text))
    }

    /// Generate a selector recipe (route B).
    pub async fn generate_recipe_json(
        &self,
        page: &Preprocessed,
        scheme: &Value,
        opts: &RunOptions,
    ) -> Result<String> {
        let recipe_meta_schema = serde_json::json!({
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

        let user_text = format!(
            "Target schema:\n{}\n\nPage URL: {}\nPage content:\n{}",
            serde_json::to_string_pretty(scheme)?,
            page.url,
            page.content,
        );

        let request = build_text_request(
            &self.model,
            opts,
            RECIPE_SYSTEM,
            user_text,
            Some(ResponseFormat::JsonSchema {
                json_schema: ResponseFormatJsonSchema {
                    name: "LlmWebRecipe".to_string(),
                    description: None,
                    schema: recipe_meta_schema,
                    strict: None,
                },
            }),
        )?;

        let response = self
            .client
            .chat()
            .create(request)
            .await
            .map_err(|e| LlmWebError::ModelClient(format!("{e}")))?;

        let text = response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| LlmWebError::ModelClient("no content in response".into()))?;

        Ok(strip_markdown_backticks!(text))
    }
}

/// Build a chat request for page-based extraction (page content is the user message).
///
/// When `schema_for_prompt` is `Some`, the schema is also embedded in the
/// system prompt as text. Strict gateways (real OpenAI) honour `response_format`;
/// loose gateways (z.ai, OpenRouter, many proxies) often pass it through as a
/// hint at best — so the model needs to see the schema directly in the prompt
/// to reliably produce the right shape.
fn build_request(
    model: &str,
    page: &Preprocessed,
    opts: &RunOptions,
    default_system: &str,
    schema_for_prompt: Option<&Value>,
    response_format: Option<ResponseFormat>,
    stream: bool,
) -> Result<CreateChatCompletionRequest> {
    let base_system = opts.system.as_deref().unwrap_or(default_system);
    let system_text = match schema_for_prompt {
        Some(schema) => format!(
            "{base_system}\n\nThe response MUST be a single JSON value that strictly matches this JSON Schema:\n{}\n\nReturn ONLY the JSON value. Do not omit wrapper keys. Do not add commentary.",
            serde_json::to_string_pretty(schema).unwrap_or_default()
        ),
        None => base_system.to_string(),
    };
    let system_msg = ChatCompletionRequestSystemMessageArgs::default()
        .content(system_text)
        .build()
        .map_err(|e| LlmWebError::ModelClient(format!("build system msg: {e}")))?;

    let user_msg = user_message_for_page(page);

    let mut builder = CreateChatCompletionRequestArgs::default();
    builder.model(model).messages(vec![system_msg.into(), user_msg.into()]);

    if stream {
        builder.stream(true);
    }
    if let Some(rf) = response_format {
        builder.response_format(rf);
    }
    if let Some(t) = opts.temperature {
        builder.temperature(t as f32);
    }
    if let Some(p) = opts.top_p {
        builder.top_p(p as f32);
    }
    if let Some(m) = opts.max_tokens {
        builder.max_completion_tokens(m);
    }

    builder
        .build()
        .map_err(|e| LlmWebError::ModelClient(format!("build request: {e}")))
}

/// Build a chat request when the user message is plain text (codegen/recipe).
fn build_text_request(
    model: &str,
    opts: &RunOptions,
    default_system: &str,
    user_text: String,
    response_format: Option<ResponseFormat>,
) -> Result<CreateChatCompletionRequest> {
    let system_text = opts.system.as_deref().unwrap_or(default_system);
    let system_msg = ChatCompletionRequestSystemMessageArgs::default()
        .content(system_text)
        .build()
        .map_err(|e| LlmWebError::ModelClient(format!("build system msg: {e}")))?;

    let user_msg = ChatCompletionRequestUserMessage {
        content: ChatCompletionRequestUserMessageContent::Text(user_text),
        name: None,
    };

    let mut builder = CreateChatCompletionRequestArgs::default();
    builder.model(model).messages(vec![system_msg.into(), user_msg.into()]);

    if let Some(rf) = response_format {
        builder.response_format(rf);
    }
    if let Some(t) = opts.temperature {
        builder.temperature(t as f32);
    }
    if let Some(p) = opts.top_p {
        builder.top_p(p as f32);
    }
    if let Some(m) = opts.max_tokens {
        builder.max_completion_tokens(m);
    }

    builder
        .build()
        .map_err(|e| LlmWebError::ModelClient(format!("build request: {e}")))
}

/// Build a user message from a `Preprocessed`. For image format the content is
/// sent as a base64-encoded `image_url` data URL; everything else is plain text.
fn user_message_for_page(page: &Preprocessed) -> ChatCompletionRequestUserMessage {
    if page.format == Format::Image {
        let text_part = ChatCompletionRequestUserMessageContentPart::Text(
            ChatCompletionRequestMessageContentPartText {
                text: "Extract structured data from the screenshot of the page below.".to_string(),
            },
        );
        let image_part = ChatCompletionRequestUserMessageContentPart::ImageUrl(
            ChatCompletionRequestMessageContentPartImage {
                image_url: ImageUrl {
                    url: format!("data:{};base64,{}", page.image_mime(), page.content),
                    detail: None,
                },
            },
        );
        ChatCompletionRequestUserMessage {
            content: ChatCompletionRequestUserMessageContent::Array(vec![text_part, image_part]),
            name: None,
        }
    } else {
        ChatCompletionRequestUserMessage {
            content: ChatCompletionRequestUserMessageContent::Text(page.content.clone()),
            name: None,
        }
    }
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
