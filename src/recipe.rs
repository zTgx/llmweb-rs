//! Route B — declarative selector recipes.
//!
//! An [`ExtractRecipe`] is a JSON description of how to walk a page with CSS
//! selectors. It's produced once by the LLM (via `LlmWeb::generate_recipe`)
//! and afterwards executed entirely in Rust against any HTML string — no
//! browser, no LLM call, no arbitrary code execution.
//!
//! This is strictly less expressive than the JS-codegen path (route A): it
//! can't follow click handlers, can't do JS-derived state, can't run async.
//! But for the common case of "list page with repeating items" it's safer,
//! cheaper, and human-reviewable.

use {
    crate::error::{LlmWebError, Result},
    scraper::{ElementRef, Html, Selector},
    serde::{Deserialize, Serialize},
    serde_json::{Map, Value},
    std::collections::HashMap,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractRecipe {
    /// Optional CSS selector that delimits each repeating item. When set, the
    /// recipe produces a JSON array. When `None`, it produces a single JSON
    /// object built from the whole document.
    #[serde(default)]
    pub container: Option<String>,
    pub fields: HashMap<String, FieldRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldRule {
    /// CSS selector. Relative to the container element when `container` is set.
    pub selector: String,
    /// Where to pull the value from:
    /// - `"text"` (default) — `element.text()` joined
    /// - `"html"` — `element.inner_html()`
    /// - anything else — interpreted as an HTML attribute name (e.g. `"href"`)
    #[serde(default = "default_attr")]
    pub attr: String,
    /// Optional post-processing:
    /// - `"int"` — parse trimmed text as i64
    /// - `"float"` — parse trimmed text as f64
    /// - `"int_prefix"` — extract leading integer (handles "42 points" -> 42)
    #[serde(default)]
    pub parse: Option<String>,
}

fn default_attr() -> String {
    "text".to_string()
}

impl ExtractRecipe {
    /// Parse a recipe from JSON.
    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).map_err(LlmWebError::SerdeJson)
    }

    /// Apply this recipe to an HTML document, producing a [`serde_json::Value`].
    /// With `container` set, the result is a JSON array; otherwise a single object.
    pub fn apply(&self, html: &str) -> Result<Value> {
        let doc = Html::parse_document(html);

        match &self.container {
            Some(container_sel) => {
                let sel = Selector::parse(container_sel).map_err(|e| {
                    LlmWebError::Recipe(format!("invalid container selector {container_sel:?}: {e}"))
                })?;
                let mut out = Vec::new();
                for item in doc.select(&sel) {
                    out.push(Value::Object(self.extract_fields(item)?));
                }
                Ok(Value::Array(out))
            }
            None => {
                // Treat the document root as the scope.
                let root_sel = Selector::parse(":root").unwrap();
                let root = doc
                    .select(&root_sel)
                    .next()
                    .ok_or_else(|| LlmWebError::Recipe("document has no root element".into()))?;
                Ok(Value::Object(self.extract_fields(root)?))
            }
        }
    }

    fn extract_fields(&self, scope: ElementRef<'_>) -> Result<Map<String, Value>> {
        let mut obj = Map::new();
        for (name, rule) in &self.fields {
            obj.insert(name.clone(), rule.apply(scope)?);
        }
        Ok(obj)
    }
}

impl FieldRule {
    fn apply(&self, scope: ElementRef<'_>) -> Result<Value> {
        let sel = Selector::parse(&self.selector).map_err(|e| {
            LlmWebError::Recipe(format!("invalid field selector {:?}: {e}", self.selector))
        })?;

        let Some(el) = scope.select(&sel).next() else {
            return Ok(Value::Null);
        };

        let raw = match self.attr.as_str() {
            "text" => el.text().collect::<String>().trim().to_string(),
            "html" => el.inner_html(),
            other => el
                .value()
                .attr(other)
                .map(|s| s.to_string())
                .unwrap_or_default(),
        };

        if raw.is_empty() && self.parse.is_none() {
            // Distinguish "missing attribute" from empty string: keep as empty
            // string for textual fields; numerics fall through to parse below
            // and yield Null.
            return Ok(Value::String(String::new()));
        }

        match self.parse.as_deref() {
            None => Ok(Value::String(raw)),
            Some("int") => raw
                .trim()
                .parse::<i64>()
                .map(|n| Value::Number(n.into()))
                .or(Ok(Value::Null)),
            Some("float") => match raw.trim().parse::<f64>() {
                Ok(f) => Ok(serde_json::Number::from_f64(f)
                    .map(Value::Number)
                    .unwrap_or(Value::Null)),
                Err(_) => Ok(Value::Null),
            },
            Some("int_prefix") => {
                let n: String = raw.chars().take_while(|c| c.is_ascii_digit()).collect();
                n.parse::<i64>()
                    .map(|v| Value::Number(v.into()))
                    .or(Ok(Value::Null))
            }
            Some(other) => Err(LlmWebError::Recipe(format!(
                "unknown parse mode: {other:?}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_array_with_container() {
        let html = r#"
            <html><body>
              <ul>
                <li class="item"><a href="/a">Alpha</a><span class="n">1</span></li>
                <li class="item"><a href="/b">Bravo</a><span class="n">2</span></li>
              </ul>
            </body></html>
        "#;
        let recipe: ExtractRecipe = serde_json::from_value(serde_json::json!({
            "container": "li.item",
            "fields": {
                "title": { "selector": "a" },
                "url":   { "selector": "a", "attr": "href" },
                "n":     { "selector": ".n", "parse": "int" }
            }
        }))
        .unwrap();

        let out = recipe.apply(html).unwrap();
        let arr = out.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["title"], "Alpha");
        assert_eq!(arr[0]["url"], "/a");
        assert_eq!(arr[0]["n"], 1);
        assert_eq!(arr[1]["title"], "Bravo");
        assert_eq!(arr[1]["n"], 2);
    }

    #[test]
    fn int_prefix_handles_unit_suffix() {
        let html = r#"<div><span class="score">42 points</span></div>"#;
        let recipe: ExtractRecipe = serde_json::from_value(serde_json::json!({
            "fields": {
                "points": { "selector": ".score", "parse": "int_prefix" }
            }
        }))
        .unwrap();
        let out = recipe.apply(html).unwrap();
        assert_eq!(out["points"], 42);
    }
}
