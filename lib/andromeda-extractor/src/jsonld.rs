//! JSON-LD body extractor with context extraction.
//!
//! Uses `mediatype` for Content-Type validation instead of substring matching,
//! preventing false positives (e.g. `application/json-patch+json` no longer
//! matches `application/json`).

use crate::error::ExtractorRejection;
use axum::{
    body::Bytes,
    extract::{FromRequest, Request},
    http::{
        StatusCode,
        header::{CONTENT_TYPE, LINK},
    },
};
use json_ld_engine::ContextSpec;
use mediatype::{MediaTypeBuf, names};
use serde::de::DeserializeOwned;
use std::collections::HashSet;

pub struct JsonLd<T> {
    pub value: T,
    pub context: ContextSpec,
}

/// Classify the Content-Type into one of the supported NGSI-LD types.
enum ContentKind {
    /// `application/json` — exact type, no suffix
    Json,
    /// `application/ld+json` — subtype=ld, suffix=json
    LdJson,
    /// `application/merge-patch+json` — subtype=merge-patch, suffix=json
    MergePatchJson,
}

fn classify_content_type(raw: &str) -> Option<ContentKind> {
    let mt: MediaTypeBuf = raw.parse().ok()?;

    if mt.ty() != names::APPLICATION {
        return None;
    }

    // application/ld+json
    if mt.subty() == names::LD && mt.suffix() == Some(names::JSON) {
        return Some(ContentKind::LdJson);
    }

    // application/merge-patch+json
    if mt.subty() == names::MERGE_PATCH && mt.suffix() == Some(names::JSON) {
        return Some(ContentKind::MergePatchJson);
    }

    // application/json — exact, no suffix (rejects application/json-patch+json etc.)
    if mt.subty() == names::JSON && mt.suffix().is_none() {
        return Some(ContentKind::Json);
    }

    None
}

fn document_has_context(document: &serde_json::Value) -> bool {
    match document {
        serde_json::Value::Object(obj) => obj.contains_key("@context"),
        serde_json::Value::Array(items) => items.iter().any(|item| item.as_object().is_some_and(|o| o.contains_key("@context"))),
        _ => false,
    }
}

fn merge_context_values_from_document(document: &serde_json::Value) -> Option<serde_json::Value> {
    let mut merged = Vec::new();
    let mut seen = HashSet::new();

    let mut push_context = |ctx: serde_json::Value| match ctx {
        serde_json::Value::Array(items) => {
            for item in items {
                let key = serde_json::to_string(&item).unwrap_or_default();
                if seen.insert(key) {
                    merged.push(item);
                }
            }
        }
        other => {
            let key = serde_json::to_string(&other).unwrap_or_default();
            if seen.insert(key) {
                merged.push(other);
            }
        }
    };

    match document {
        serde_json::Value::Object(obj) => {
            if let Some(ctx) = obj.get("@context") {
                push_context(ctx.clone());
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                if let Some(ctx) = item.as_object().and_then(|obj| obj.get("@context")) {
                    push_context(ctx.clone());
                }
            }
        }
        _ => {}
    }

    match merged.len() {
        0 => None,
        1 => merged.into_iter().next(),
        _ => Some(serde_json::Value::Array(merged)),
    }
}

fn strip_context_from_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(obj) => {
            obj.remove("@context");
        }
        serde_json::Value::Array(items) => {
            for item in items {
                if let serde_json::Value::Object(obj) = item {
                    obj.remove("@context");
                }
            }
        }
        _ => {}
    }
}

impl<S, T> FromRequest<S> for JsonLd<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ExtractorRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let uri = req.uri().clone();
        let content_type_raw = req.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");

        let content_kind = match classify_content_type(content_type_raw) {
            Some(kind) => kind,
            None => {
                tracing::warn!("JsonLd extraction failed: unsupported media type '{}'", content_type_raw);
                return Err(ExtractorRejection::Empty(StatusCode::UNSUPPORTED_MEDIA_TYPE));
            }
        };

        let is_ld_json = matches!(content_kind, ContentKind::LdJson);
        let is_plain_json = matches!(content_kind, ContentKind::Json);

        // Per NGSI-LD 6.3.5: application/ld+json MUST NOT be combined with a Link header
        // containing a JSON-LD context relation
        if is_ld_json {
            let has_context_link = req
                .headers()
                .get_all(LINK)
                .iter()
                .any(|v| v.to_str().ok().map(|s| s.contains("http://www.w3.org/ns/json-ld#context")).unwrap_or(false));
            if has_context_link {
                return Err(ExtractorRejection::bad_request_data(
                    "application/ld+json must not be combined with a Link context header",
                    uri,
                ));
            }
        }

        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|e| ExtractorRejection::invalid_request(format!("Invalid request body: {}", e), uri.clone()))?;

        if bytes.is_empty() {
            return Err(ExtractorRejection::invalid_request("Request body shall not be empty", uri.clone()));
        }

        // Parse as Value first to extract @context if needed, then strip
        let mut value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| ExtractorRejection::invalid_request(format!("Invalid JSON: {}", e), uri.clone()))?;

        // Reject empty arrays — not a valid NGSI-LD entity or batch payload
        if value.as_array().is_some_and(|a| a.is_empty()) {
            return Err(ExtractorRejection::invalid_request(
                "Invalid JSON: expected object, got empty array",
                uri.clone(),
            ));
        }

        // Per NGSI-LD 6.3.5: application/json MUST NOT contain @context in body
        if is_plain_json && document_has_context(&value) {
            return Err(ExtractorRejection::bad_request_data("application/json body must not contain @context", uri));
        }

        // Per NGSI-LD 6.3.5: application/ld+json MUST contain @context in body
        if is_ld_json && !document_has_context(&value) {
            return Err(ExtractorRejection::bad_request_data("application/ld+json body must contain @context", uri));
        }

        let mut context = ContextSpec::none();
        if is_ld_json {
            if let Some(ctx) = merge_context_values_from_document(&value) {
                context = context.with_body_context(Some(ctx));
            }
        }

        // Always strip @context before deserializing into Entity
        // Context is handled separately by ContextUrl extractor
        strip_context_from_value(&mut value);

        let typed_value: T = serde_json::from_value(value).map_err(|e| ExtractorRejection::invalid_request(format!("Invalid JSON: {}", e), uri.clone()))?;

        Ok(JsonLd { value: typed_value, context })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use serde_json::Value;

    #[tokio::test]
    async fn rejects_unsupported_media_type() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/xml")
            .body(axum::body::Body::from("{}"))
            .unwrap();

        let result = JsonLd::<Value>::from_request(req, &()).await;
        match result {
            Err(ExtractorRejection::Empty(status)) => {
                assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
            }
            _ => panic!("expected 415 empty rejection"),
        }
    }

    #[tokio::test]
    async fn rejects_json_patch_as_unsupported() {
        // application/json-patch+json must NOT be accepted as application/json
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/json-patch+json")
            .body(axum::body::Body::from("{}"))
            .unwrap();

        let result = JsonLd::<Value>::from_request(req, &()).await;
        match result {
            Err(ExtractorRejection::Empty(status)) => {
                assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
            }
            _ => panic!("expected 415 for json-patch+json"),
        }
    }

    #[tokio::test]
    async fn accepts_merge_patch_json() {
        let body = serde_json::json!({"id": "urn:ngsi-ld:T:1", "type": "T"});
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:T:1")
            .header(CONTENT_TYPE, "application/merge-patch+json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let result = JsonLd::<Value>::from_request(req, &()).await;
        assert!(result.is_ok(), "merge-patch+json should be accepted");
    }

    #[tokio::test]
    async fn rejects_empty_body_with_bad_request() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/json")
            .body(axum::body::Body::empty())
            .unwrap();

        let result = JsonLd::<Value>::from_request(req, &()).await;
        assert!(matches!(result, Err(ExtractorRejection::InvalidRequest { .. })));
    }

    #[tokio::test]
    async fn extracts_context_from_body() {
        let body = serde_json::json!({
            "@context": "http://example.org/context",
            "id": "urn:ngsi-ld:T:1",
            "type": "T"
        });
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/ld+json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let result = JsonLd::<Value>::from_request(req, &()).await.unwrap();
        assert_eq!(result.context.body_context(), Some(&serde_json::json!("http://example.org/context")));
        assert!(result.value.as_object().unwrap().get("@context").is_none());
    }

    #[tokio::test]
    async fn extracts_and_merges_contexts_from_array_body() {
        let body = serde_json::json!([
            {
                "@context": "https://example.org/a",
                "id": "urn:ngsi-ld:T:1",
                "type": "T"
            },
            {
                "@context": ["https://example.org/b", "https://example.org/a"],
                "id": "urn:ngsi-ld:T:2",
                "type": "T"
            }
        ]);

        let req = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/create")
            .header(CONTENT_TYPE, "application/ld+json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let result = JsonLd::<Value>::from_request(req, &()).await.unwrap();
        assert_eq!(
            result.context.body_context(),
            Some(&serde_json::json!(["https://example.org/a", "https://example.org/b"]))
        );
    }

    #[tokio::test]
    async fn content_type_with_charset_param_works() {
        let body = serde_json::json!({"id": "urn:ngsi-ld:T:1", "type": "T"});
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let result = JsonLd::<Value>::from_request(req, &()).await;
        assert!(result.is_ok());
    }
}
