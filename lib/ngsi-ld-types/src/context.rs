use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// A JSON-LD `@context` value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum NgsiLdContext {
    String(String),
    List(Vec<String>),
    Map(IndexMap<String, String>),
}

impl Default for NgsiLdContext {
    fn default() -> Self {
        NgsiLdContext::List(vec!["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld".to_string()])
    }
}

impl NgsiLdContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a context URL to this context value.
    pub fn add_context(&mut self, context: String) {
        match self {
            NgsiLdContext::String(s) => {
                *self = NgsiLdContext::List(vec![s.clone(), context]);
            }
            NgsiLdContext::List(v) => {
                if !v.contains(&context) {
                    v.push(context);
                }
            }
            NgsiLdContext::Map(_) => {
                *self = NgsiLdContext::List(vec![context]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_context_is_core() {
        let ctx = NgsiLdContext::default();
        match ctx {
            NgsiLdContext::List(v) => {
                assert_eq!(v.len(), 1);
                assert!(v[0].contains("ngsi-ld-core-context"));
            }
            _ => panic!("Expected List"),
        }
    }

    #[test]
    fn add_to_string_promotes_to_list() {
        let mut ctx = NgsiLdContext::String("https://schema.org".to_string());
        ctx.add_context("https://example.com/context.jsonld".to_string());
        match ctx {
            NgsiLdContext::List(v) => assert_eq!(v.len(), 2),
            _ => panic!("Expected List"),
        }
    }

    #[test]
    fn add_duplicate_is_noop() {
        let mut ctx = NgsiLdContext::List(vec!["https://schema.org".to_string()]);
        ctx.add_context("https://schema.org".to_string());
        match ctx {
            NgsiLdContext::List(v) => assert_eq!(v.len(), 1),
            _ => panic!("Expected List"),
        }
    }

    #[test]
    fn string_context_round_trips() {
        let ctx = NgsiLdContext::String("https://schema.org".to_string());
        let json = serde_json::to_string(&ctx).unwrap();
        let parsed: NgsiLdContext = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ctx);
    }

    #[test]
    fn list_context_round_trips() {
        let ctx = NgsiLdContext::List(vec!["https://schema.org".to_string(), "https://example.com/ctx".to_string()]);
        let json = serde_json::to_string(&ctx).unwrap();
        let parsed: NgsiLdContext = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ctx);
    }
}
