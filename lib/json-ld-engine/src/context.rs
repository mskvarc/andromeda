//! Context specification and resolution for JSON-LD operations.
//!
//! [`ContextSpec`] describes which JSON-LD contexts apply to a request
//! (Link-header contexts, body contexts, core-context policy) and provides
//! helpers for merging, resolving, and formatting them.

use crate::{
    Error,
    core_context::{NGSI_LD_CORE_CONTEXT_URL, is_ngsi_ld_core_context_url},
};
use serde_json::Value;
use std::{collections::HashSet, sync::OnceLock};

/// Global default core context URL override (set once at startup).
static DEFAULT_CORE_CONTEXT_URL: OnceLock<String> = OnceLock::new();

/// Set the global default core context URL. Call once at startup.
pub fn set_default_core_context_url(url: impl Into<String>) {
    let _ = DEFAULT_CORE_CONTEXT_URL.set(url.into());
}

/// Get the global default core context URL.
pub fn default_core_context_url() -> &'static str {
    DEFAULT_CORE_CONTEXT_URL.get().map(String::as_str).unwrap_or(NGSI_LD_CORE_CONTEXT_URL)
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Strategy for merging document and Link-header contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMerge {
    /// Document context first, then Link contexts.
    PreferLink,
    /// Link contexts first, then document context.
    PreferDocument,
}

/// Policy for how the NGSI-LD core context is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreContextPolicy {
    /// Always append the core context last.
    Append,
    /// Append only when no explicit (link / body) context is present.
    AppendIfNoExplicit,
    /// Do not add the core context.
    Omit,
    /// Use only the core context; discard all others.
    Only,
}

/// Describes the JSON-LD contexts that apply to a single operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSpec {
    link_contexts: Vec<String>,
    body_context: Option<Value>,
    merge: ContextMerge,
    core_policy: CoreContextPolicy,
    /// Override the default core context URL (e.g. to use a specific version).
    default_core_url: Option<String>,
}

impl ContextSpec {
    /// Empty spec with the default core-context append policy.
    pub fn none() -> Self {
        let global_default = DEFAULT_CORE_CONTEXT_URL.get().cloned();
        Self {
            link_contexts: Vec::new(),
            body_context: None,
            merge: ContextMerge::PreferLink,
            core_policy: CoreContextPolicy::Append,
            default_core_url: global_default,
        }
    }

    /// Spec that resolves to only the core context.
    pub fn core_only() -> Self {
        let global_default = DEFAULT_CORE_CONTEXT_URL.get().cloned();
        Self {
            link_contexts: Vec::new(),
            body_context: None,
            merge: ContextMerge::PreferLink,
            core_policy: CoreContextPolicy::Only,
            default_core_url: global_default,
        }
    }

    /// Spec built from Link-header context URLs.
    pub fn from_links(links: Vec<String>) -> Self {
        let global_default = DEFAULT_CORE_CONTEXT_URL.get().cloned();
        Self {
            link_contexts: links,
            body_context: None,
            merge: ContextMerge::PreferLink,
            core_policy: CoreContextPolicy::Append,
            default_core_url: global_default,
        }
    }

    /// Set the body context.
    pub fn with_body_context(mut self, context: Option<Value>) -> Self {
        self.body_context = context;
        self
    }

    /// Get the body context, if any.
    pub fn body_context(&self) -> Option<&Value> {
        self.body_context.as_ref()
    }

    /// Append a Link-header context URL.
    pub fn push_link(&mut self, url: impl Into<String>) {
        self.link_contexts.push(url.into());
    }

    /// Link-header context URLs in insertion order.
    pub fn link_contexts(&self) -> &[String] {
        &self.link_contexts
    }

    /// Set the merge strategy.
    pub fn with_merge(mut self, merge: ContextMerge) -> Self {
        self.merge = merge;
        self
    }

    /// Set the core-context policy.
    pub fn with_core_policy(mut self, policy: CoreContextPolicy) -> Self {
        self.core_policy = policy;
        self
    }

    /// Current merge strategy.
    pub fn merge(&self) -> ContextMerge {
        self.merge
    }

    /// Current core-context policy.
    pub fn core_policy(&self) -> CoreContextPolicy {
        self.core_policy
    }

    /// Set the default core context URL override.
    pub fn with_default_core_url(mut self, url: impl Into<String>) -> Self {
        self.default_core_url = Some(url.into());
        self
    }

    /// The effective core context URL (configured default or versionless fallback).
    pub fn effective_core_url(&self) -> &str {
        self.default_core_url.as_deref().unwrap_or(NGSI_LD_CORE_CONTEXT_URL)
    }

    /// Create a presentation-mode copy.
    ///
    /// Uses [`CoreContextPolicy::AppendIfNoExplicit`] so the core context is
    /// included only when no explicit external context was provided.
    pub fn for_presentation(&self) -> Self {
        Self {
            link_contexts: self.link_contexts.clone(),
            body_context: self.body_context.clone(),
            merge: self.merge,
            core_policy: CoreContextPolicy::AppendIfNoExplicit,
            default_core_url: self.default_core_url.clone(),
        }
    }

    /// Resolve to a `@context` JSON value for embedding in responses.
    pub fn to_context_value(&self) -> Option<Value> {
        resolve_context_value(None, self)
    }

    /// Build the ordered list of context URLs for `Link` headers.
    pub fn to_link_headers(&self) -> Vec<String> {
        let mut urls = Vec::new();
        let mut seen = HashSet::new();

        let core_url = self.effective_core_url().to_string();

        match self.core_policy {
            CoreContextPolicy::Only => {
                urls.push(core_url);
                return urls;
            }
            CoreContextPolicy::Omit => {}
            CoreContextPolicy::Append | CoreContextPolicy::AppendIfNoExplicit => {}
        }

        let should_strip_core = matches!(self.core_policy, CoreContextPolicy::Append | CoreContextPolicy::AppendIfNoExplicit);

        for url in &self.link_contexts {
            if should_strip_core && is_ngsi_ld_core_context_url(url) {
                continue;
            }
            if seen.insert(url.clone()) {
                urls.push(url.clone());
            }
        }

        let has_explicit = !urls.is_empty() || self.body_context.is_some();
        let should_append_core = match self.core_policy {
            CoreContextPolicy::Append => true,
            CoreContextPolicy::AppendIfNoExplicit => !has_explicit,
            _ => false,
        };

        if should_append_core && seen.insert(core_url.clone()) {
            urls.push(core_url);
        }

        urls
    }
}

impl Default for ContextSpec {
    fn default() -> Self {
        Self::none()
    }
}

// ---------------------------------------------------------------------------
// Resolution helpers
// ---------------------------------------------------------------------------

/// Resolve a `@context` value by combining document and [`ContextSpec`] inputs.
pub fn resolve_context_value(document_context: Option<&Value>, spec: &ContextSpec) -> Option<Value> {
    let mut doc_items = extract_context_items(document_context);
    if let Some(body_ctx) = spec.body_context() {
        doc_items.extend(extract_context_items(Some(body_ctx)));
    }

    let mut link_items: Vec<Value> = spec.link_contexts().iter().cloned().map(Value::String).collect();

    let mut combined = match spec.merge() {
        ContextMerge::PreferLink => {
            doc_items.append(&mut link_items);
            doc_items
        }
        ContextMerge::PreferDocument => {
            link_items.append(&mut doc_items);
            link_items
        }
    };

    combined = dedupe_contexts(combined);

    match spec.core_policy() {
        CoreContextPolicy::Only => {
            return Some(Value::String(spec.effective_core_url().to_string()));
        }
        CoreContextPolicy::Omit => {}
        CoreContextPolicy::Append => {
            combined.retain(|v| !v.as_str().is_some_and(is_ngsi_ld_core_context_url));
            combined.push(Value::String(spec.effective_core_url().to_string()));
        }
        CoreContextPolicy::AppendIfNoExplicit => {
            combined.retain(|v| !v.as_str().is_some_and(is_ngsi_ld_core_context_url));
            if combined.is_empty() {
                combined.push(Value::String(spec.effective_core_url().to_string()));
            }
        }
    }

    match combined.len() {
        0 => None,
        1 => Some(combined.into_iter().next().unwrap()),
        _ => Some(Value::Array(combined)),
    }
}

/// Apply the resolved context to a JSON document.
pub fn apply_context(document: Value, spec: &ContextSpec) -> Result<Value, Error> {
    match document {
        Value::Object(mut obj) => {
            let doc_context = obj.get("@context").cloned();
            let resolved = resolve_context_value(doc_context.as_ref(), spec);
            match resolved {
                Some(ctx) => {
                    obj.insert("@context".to_string(), ctx);
                }
                None => {
                    obj.remove("@context");
                }
            }
            Ok(Value::Object(obj))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Value::Object(mut obj) => {
                        let doc_context = obj.get("@context").cloned();
                        let resolved = resolve_context_value(doc_context.as_ref(), spec);
                        match resolved {
                            Some(ctx) => {
                                obj.insert("@context".to_string(), ctx);
                            }
                            None => {
                                obj.remove("@context");
                            }
                        }
                        out.push(Value::Object(obj));
                    }
                    other => out.push(other),
                }
            }
            Ok(Value::Array(out))
        }
        _ => Err(Error::invalid_document("Document must be an object or array")),
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn extract_context_items(context: Option<&Value>) -> Vec<Value> {
    match context {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::Null) => Vec::new(),
        Some(other) => vec![other.clone()],
        None => Vec::new(),
    }
}

fn dedupe_contexts(contexts: Vec<Value>) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(contexts.len());
    for value in contexts {
        if let Value::String(ref s) = value {
            if seen.insert(s.clone()) {
                out.push(value);
            }
        } else {
            out.push(value);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn context_spec_defaults_include_core() {
        let spec = ContextSpec::none();
        let ctx = spec.to_context_value().expect("context value");
        assert_eq!(ctx, json!(NGSI_LD_CORE_CONTEXT_URL));
    }

    #[test]
    fn context_spec_link_headers_include_core() {
        let spec = ContextSpec::from_links(vec!["https://example.com/context.jsonld".to_string()]);
        let links = spec.to_link_headers();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0], "https://example.com/context.jsonld");
        assert_eq!(links[1], NGSI_LD_CORE_CONTEXT_URL);
    }

    #[test]
    fn context_spec_only_core_policy() {
        let spec = ContextSpec::from_links(vec!["https://example.com/context.jsonld".to_string()]).with_core_policy(CoreContextPolicy::Only);
        let links = spec.to_link_headers();
        assert_eq!(links, vec![NGSI_LD_CORE_CONTEXT_URL.to_string()]);
    }

    #[test]
    fn context_spec_core_only() {
        let spec = ContextSpec::core_only();
        let ctx = spec.to_context_value().expect("context value");
        assert_eq!(ctx, json!(NGSI_LD_CORE_CONTEXT_URL));
        assert_eq!(spec.core_policy(), CoreContextPolicy::Only);
    }

    #[test]
    fn context_spec_link_headers_normalize_core_aliases() {
        let spec = ContextSpec::from_links(vec![
            "https://example.com/context.jsonld".to_string(),
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld".to_string(),
        ]);
        let links = spec.to_link_headers();
        assert_eq!(
            links,
            vec!["https://example.com/context.jsonld".to_string(), NGSI_LD_CORE_CONTEXT_URL.to_string()]
        );
    }

    #[test]
    fn resolve_context_merges_and_appends_core_last() {
        let spec = ContextSpec::from_links(vec!["https://example.com/a".to_string()]);
        let doc_ctx = json!(["https://example.com/doc", "https://example.com/a"]);
        let resolved = resolve_context_value(Some(&doc_ctx), &spec).expect("context");

        assert_eq!(resolved, json!(["https://example.com/doc", "https://example.com/a", NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn prefer_document_keeps_link_first_with_core_last() {
        let spec = ContextSpec::from_links(vec!["https://example.com/a".to_string()]).with_merge(ContextMerge::PreferDocument);
        let doc_ctx = json!("https://example.com/doc");
        let resolved = resolve_context_value(Some(&doc_ctx), &spec).expect("context");

        assert_eq!(resolved, json!(["https://example.com/a", "https://example.com/doc", NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn appends_core_when_no_explicit_context_exists() {
        let spec = ContextSpec::none();
        let resolved = resolve_context_value(None, &spec).expect("context");
        assert_eq!(resolved, json!(NGSI_LD_CORE_CONTEXT_URL));
    }

    #[test]
    fn core_alias_is_normalized_to_canonical_core_last() {
        let spec = ContextSpec::none();
        let doc_ctx = json!([
            "https://example.com/compound.jsonld",
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld"
        ]);
        let resolved = resolve_context_value(Some(&doc_ctx), &spec).expect("context");

        assert_eq!(resolved, json!(["https://example.com/compound.jsonld", NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn body_context_keeps_value_and_appends_core_last() {
        let spec = ContextSpec::none().with_body_context(Some(json!("https://example.com/compound.jsonld")));
        let resolved = resolve_context_value(None, &spec).expect("context");

        assert_eq!(resolved, json!(["https://example.com/compound.jsonld", NGSI_LD_CORE_CONTEXT_URL]));
    }

    #[test]
    fn apply_context_to_object() {
        let spec = ContextSpec::none();
        let doc = json!({"id": "urn:test:1", "type": "Test"});
        let result = apply_context(doc, &spec).unwrap();
        assert!(result.get("@context").is_some());
    }

    #[test]
    fn apply_context_to_array() {
        let spec = ContextSpec::none();
        let doc = json!([{"id": "urn:test:1", "type": "Test"}]);
        let result = apply_context(doc, &spec).unwrap();
        assert!(result[0].get("@context").is_some());
    }

    #[test]
    fn apply_context_rejects_non_object_non_array() {
        let spec = ContextSpec::none();
        let result = apply_context(json!("string"), &spec);
        assert!(result.is_err());
    }
}
