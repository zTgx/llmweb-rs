//! Incremental JSON streaming.
//!
//! `genai` emits raw text chunks as the LLM generates tokens. This module
//! turns that token stream into a `Stream<Item = Result<R>>` of progressively
//! more-complete parsed values.
//!
//! The trick is "repairing" the partial JSON buffer at each tick:
//! - close any open string,
//! - drop a dangling trailing comma,
//! - replace a key-without-value with `null`,
//! - append matching `}` / `]` for every unclosed container.
//!
//! The repaired snapshot is then fed to `serde_json::from_str::<R>`. If `R`
//! has only `Option<...>` fields, every snapshot deserializes; otherwise the
//! early snapshots fail until enough required fields have arrived, and the
//! stream skips those ticks silently.

use {
    crate::error::{LlmWebError, Result},
    async_stream::try_stream,
    futures::{Stream, StreamExt},
    genai::chat::{ChatStream, ChatStreamEvent},
    serde::de::DeserializeOwned,
    std::pin::Pin,
};

pub type PartialStream<R> = Pin<Box<dyn Stream<Item = Result<R>> + Send>>;

/// Wrap a raw genai `ChatStream` into a `Stream<Item = Result<R>>` that yields
/// a fresh `R` every time the partial buffer grows into something parseable.
/// Duplicate consecutive snapshots are filtered.
pub fn partial_stream<R>(mut chat: ChatStream) -> PartialStream<R>
where
    R: DeserializeOwned + Send + 'static + PartialEq,
{
    let s = try_stream! {
        let mut buf = String::new();
        let mut last: Option<R> = None;

        while let Some(event) = chat.next().await {
            let event = event.map_err(|e| LlmWebError::ModelClient(format!("{e}")))?;
            let chunk = match event {
                ChatStreamEvent::Chunk(c) => c.content,
                // Ignore Start / ReasoningChunk / End — only assistant text matters.
                _ => continue,
            };
            if chunk.is_empty() {
                continue;
            }
            buf.push_str(&chunk);

            let repaired = repair_partial_json(&buf);
            let Ok(value) = serde_json::from_str::<R>(&repaired) else {
                continue;
            };
            if last.as_ref() == Some(&value) {
                continue;
            }
            last = Some(value);
            // Re-parse for the yielded copy (R isn't Clone in general).
            yield serde_json::from_str::<R>(&repaired)?;
        }
    };
    Box::pin(s)
}

/// Repair a possibly-truncated JSON string into something `serde_json` can parse.
///
/// Guarantees: the result is a well-formed JSON document IF the input was a
/// prefix of one. Otherwise it returns a best-effort repair that may still
/// fail to parse — callers should treat parse failures as "wait for more data".
pub fn repair_partial_json(input: &str) -> String {
    let mut stack: Vec<u8> = Vec::new();
    let mut in_string = false;
    let mut escape = false;
    // Track whether we're inside an object key vs an object value.
    // A bit per open object: true = expecting value after a `:`, false = expecting key/colon.
    // We only need this for the *current* (innermost) object, but tracking
    // per-frame keeps things consistent across nested structures.
    let mut expecting_value: Vec<bool> = Vec::new();

    for ch in input.bytes() {
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            match ch {
                b'\\' => escape = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            b'"' => in_string = true,
            b'{' => {
                stack.push(b'}');
                expecting_value.push(false);
            }
            b'[' => {
                stack.push(b']');
                expecting_value.push(true); // arrays only contain values
            }
            b'}' | b']' => {
                stack.pop();
                expecting_value.pop();
            }
            b':' => {
                if let Some(last) = expecting_value.last_mut() {
                    *last = true;
                }
            }
            b',' => {
                if let Some(last) = expecting_value.last_mut() {
                    // After a comma in an object we expect a new key again;
                    // in an array we still expect a value.
                    if *stack.last().unwrap_or(&b'?') == b'}' {
                        *last = false;
                    }
                }
            }
            _ => {}
        }
    }

    let mut out = input.trim_end().to_string();

    // Close an open string. If we were mid-key, the closed string is a
    // complete key — but a key without colon+value is invalid, so we'll
    // strip it below.
    if in_string {
        out.push('"');
    }

    // Walk back over trailing junk that can't terminate a valid JSON document:
    //   - trailing whitespace
    //   - trailing `,`
    //   - trailing `:` (gets a `null` value)
    //   - an object's dangling key (string literal not followed by `:`)
    loop {
        let trimmed_len = out.trim_end().len();
        out.truncate(trimmed_len);
        let Some(last) = out.chars().last() else { break };

        match last {
            ',' => {
                out.pop();
                continue;
            }
            ':' => {
                out.push_str("null");
                break;
            }
            _ => {}
        }

        // Detect "object expecting a key, but we have a dangling complete
        // string with no colon" — drop the string. e.g. `{"foo` after we
        // closed the string becomes `{"foo"`, which is an invalid object.
        let in_object = stack.last() == Some(&b'}');
        let expecting_v = expecting_value.last().copied().unwrap_or(false);
        if in_object && !expecting_v && last == '"' {
            // Strip the trailing string literal (back to the opening quote).
            if let Some(idx) = find_unescaped_quote_from_end(&out) {
                out.truncate(idx);
                continue;
            }
        }
        break;
    }

    // Append matching closers in reverse-open order.
    for closer in stack.iter().rev() {
        out.push(*closer as char);
    }
    out
}

/// Given a string whose final char is `"`, find the byte index of the
/// matching opening `"` (i.e. the start of the trailing string literal),
/// respecting `\"` escapes.
fn find_unescaped_quote_from_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes[bytes.len() - 1] != b'"' {
        return None;
    }
    // Walk backwards from the position before the trailing quote.
    let mut i = bytes.len().checked_sub(1)?;
    while i > 0 {
        i -= 1;
        if bytes[i] == b'"' {
            // Count consecutive backslashes before this quote.
            let mut bs = 0usize;
            let mut j = i;
            while j > 0 && bytes[j - 1] == b'\\' {
                bs += 1;
                j -= 1;
            }
            if bs % 2 == 0 {
                return Some(i);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn parse(input: &str) -> Value {
        let repaired = repair_partial_json(input);
        serde_json::from_str(&repaired)
            .unwrap_or_else(|e| panic!("repaired {repaired:?} failed to parse: {e}"))
    }

    #[test]
    fn closes_open_object() {
        assert_eq!(parse(r#"{"a": 1"#), json!({"a": 1}));
    }

    #[test]
    fn closes_open_array() {
        assert_eq!(parse(r#"[1, 2, 3"#), json!([1, 2, 3]));
    }

    #[test]
    fn drops_trailing_comma() {
        assert_eq!(parse(r#"[1, 2,"#), json!([1, 2]));
        assert_eq!(parse(r#"{"a": 1,"#), json!({"a": 1}));
    }

    #[test]
    fn fills_dangling_colon_with_null() {
        assert_eq!(parse(r#"{"a":"#), json!({"a": null}));
    }

    #[test]
    fn closes_open_string_in_value_position() {
        assert_eq!(parse(r#"{"a": "hel"#), json!({"a": "hel"}));
    }

    #[test]
    fn drops_dangling_partial_key() {
        // `{"a": 1, "b` — repair to `{"a": 1}`, dropping the half-typed key.
        assert_eq!(parse(r#"{"a": 1, "b"#), json!({"a": 1}));
    }

    #[test]
    fn drops_dangling_complete_key_without_colon() {
        // `{"a": 1, "b"` — same as above; we know key is complete but no colon.
        assert_eq!(parse(r#"{"a": 1, "b""#), json!({"a": 1}));
    }

    #[test]
    fn handles_nested() {
        assert_eq!(
            parse(r#"{"top": [{"title": "hi", "n": 1}, {"title": "two"#),
            json!({"top": [{"title": "hi", "n": 1}, {"title": "two"}]})
        );
    }

    #[test]
    fn escapes_inside_strings_ignored() {
        // The `{` inside the string must NOT push another frame.
        assert_eq!(parse(r#"{"a": "x{y"#), json!({"a": "x{y"}));
    }

    #[test]
    fn passes_through_already_valid_json() {
        let s = r#"{"a": 1, "b": [2, 3]}"#;
        assert_eq!(repair_partial_json(s), s);
    }
}
