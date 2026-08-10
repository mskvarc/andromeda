//! Batch entity request parsing for NGSI-LD batch operations.
//!
//! Provides [`BatchBody`] (a [`FromRequest`] extractor that validates Content-Type)
//! and [`parse_batch_entities`] which handles per-entity JSON-LD context merging,
//! content-type classification, and validation.

use crate::context::ContextUrl;
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri as EntityId;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchEntityError, error_type::NgsiLdErrorType};
use axum::{
    body::Bytes,
    extract::{FromRequest, Request},
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;
use mediatype::{MediaTypeBuf, names};
use serde_json::Value;
use std::collections::HashSet;

/// Construct a deliberate public batch-extraction failure with request instance.
fn public_error(error_type: NgsiLdErrorType, detail: impl Into<String>, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::public(error_type, detail).with_instance(instance.clone())
}

/// Parse a batch request body into a JSON `Value` via `sonic-rs`.
///
/// `sonic-rs` accepts `&[u8]` directly (no mutation, no copy) and runtime-
/// dispatches its SIMD implementation (AVX2 / NEON / scalar) based on host
/// CPU features. Used unconditionally on the batch path: the dispatch cost
/// is amortised over header inspection and per-entity context merging that
/// already happen above and below the parse, so even sub-KiB bodies prefer
/// it over `serde_json`'s byte-by-byte scan.
fn parse_batch_body_json(body: &Bytes) -> Result<Value, String> {
    sonic_rs::from_slice::<Value>(body).map_err(|err| err.to_string())
}

/// Content-Type classification for batch entity parsing.
enum ContentKind {
    /// Plain `application/json` batch representation.
    Json,
    /// JSON-LD `application/ld+json` batch representation.
    LdJson,
}

/// Classify one raw content type into a supported batch representation.
fn classify_batch_content_type(raw: &str) -> Option<ContentKind> {
    let mt: MediaTypeBuf = raw.parse().ok()?;
    if mt.ty() != names::APPLICATION {
        return None;
    }
    if mt.subty() == names::LD && mt.suffix() == Some(names::JSON) {
        return Some(ContentKind::LdJson);
    }
    if mt.subty() == names::JSON && mt.suffix().is_none() {
        return Some(ContentKind::Json);
    }
    None
}

/// Return whether the request headers select a supported batch content type.
fn has_supported_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .typed_get::<headers::ContentType>()
        .is_some_and(|value| classify_batch_content_type(&value.to_string()).is_some())
}

/// Validated batch request body with associated headers.
///
/// Bundles the request headers and body bytes for batch entity operations,
/// rejecting the request with 415 Unsupported Media Type if the Content-Type
/// is not a supported JSON variant (`application/json` or `application/ld+json`).
pub struct BatchBody {
    /// Request headers, needed by [`parse_batch_entities`] for Content-Type
    /// classification and Link header conflict detection.
    pub headers: HeaderMap,
    /// Raw request body bytes.
    pub body: Bytes,
}

impl<S> FromRequest<S> for BatchBody
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<BatchBody, Self::Rejection> {
        let (mut parts, body) = req.into_parts();
        if !has_supported_json_content_type(&parts.headers) {
            return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
        }
        // Move headers out before reassembling the request for body extraction.
        // Bytes::from_request does not inspect headers, so empty headers are safe.
        let headers = std::mem::take(&mut parts.headers);
        let req = Request::from_parts(parts, body);
        let body = Bytes::from_request(req, state).await.map_err(IntoResponse::into_response)?;
        Ok(BatchBody { headers, body })
    }
}

/// Parse a batch request body with per-entity error handling.
///
/// Returns `(valid_entities, context, per_entity_errors)`.
/// Whole-request errors (empty body, invalid JSON, unsupported Content-Type)
/// are returned as `Err`.
pub fn parse_batch_entities(
    body: &Bytes,
    headers: &HeaderMap,
    context: &ContextUrl,
    uri: &Uri,
) -> Result<(Vec<Entity>, ContextSpec, Vec<BatchEntityError>), ProblemResponse> {
    let content_type_raw = headers.typed_get::<headers::ContentType>().map(|value| value.to_string()).unwrap_or_default();

    let content_kind = classify_batch_content_type(&content_type_raw)
        .ok_or_else(|| public_error(NgsiLdErrorType::InvalidRequest, format!("Unsupported Media Type: {content_type_raw}"), uri))?;

    let is_ld_json = matches!(content_kind, ContentKind::LdJson);
    let is_plain_json = matches!(content_kind, ContentKind::Json);

    // Check for conflicting Link header + ld+json
    if is_ld_json {
        let has_context_link = headers
            .get_all("Link")
            .iter()
            .any(|v| v.to_str().ok().is_some_and(|s| s.contains("http://www.w3.org/ns/json-ld#context")));
        if has_context_link {
            return Err(public_error(
                NgsiLdErrorType::InvalidRequest,
                "application/ld+json must not be combined with a Link context header",
                uri,
            ));
        }
    }

    if body.is_empty() {
        return Err(public_error(NgsiLdErrorType::InvalidRequest, "Request body shall not be empty", uri));
    }

    let value: Value = parse_batch_body_json(body).map_err(|error| public_error(NgsiLdErrorType::InvalidRequest, format!("Invalid JSON: {error}"), uri))?;

    let items = match value {
        Value::Array(arr) if arr.is_empty() => {
            return Err(public_error(
                NgsiLdErrorType::InvalidRequest,
                "Request body must contain a non-empty array of entities",
                uri,
            ));
        }
        Value::Array(arr) => arr,
        _ => {
            return Err(public_error(
                NgsiLdErrorType::InvalidRequest,
                "Request body must be a JSON array of entities",
                uri,
            ));
        }
    };

    // Merge all @context values from all items for the overall context
    let mut merged_context = context.0.clone(); // clone: need owned ContextSpec to merge body contexts into
    if is_ld_json {
        let mut seen = HashSet::new();
        let mut merged_ctx_values = Vec::new();
        for item in &items {
            if let Some(ctx) = item.as_object().and_then(|o| o.get("@context")) {
                match ctx {
                    Value::Array(arr) => {
                        for v in arr {
                            let key = v.to_string();
                            if seen.insert(key) {
                                merged_ctx_values.push(v.clone()); // clone: collecting values from borrowed JSON
                            }
                        }
                    }
                    other => {
                        let key = other.to_string();
                        if seen.insert(key) {
                            merged_ctx_values.push(other.clone()); // clone: collecting values from borrowed JSON
                        }
                    }
                }
            }
        }
        if !merged_ctx_values.is_empty() {
            let ctx_value = if merged_ctx_values.len() == 1 {
                match merged_ctx_values.into_iter().next() {
                    Some(value) => value,
                    None => Value::Array(Vec::new()),
                }
            } else {
                Value::Array(merged_ctx_values)
            };
            merged_context = merged_context.with_body_context(Some(ctx_value));
        }
    }

    let mut entities = Vec::new();
    let mut parse_errors = Vec::new();

    for mut item in items {
        let entity_id = item
            .as_object()
            .and_then(|o| o.get("id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| public_error(NgsiLdErrorType::InvalidRequest, "Every batch entity must contain a valid URI id", uri))?
            .parse::<EntityId>()
            .map_err(|_error| public_error(NgsiLdErrorType::InvalidRequest, "Every batch entity must contain a valid URI id", uri))?;

        // Per-entity @context validation
        let has_context = item.as_object().is_some_and(|o| o.contains_key("@context"));

        if is_plain_json && has_context {
            parse_errors.push(BatchEntityError::new(
                entity_id,
                NgsiLdErrorType::BadRequestData,
                "application/json body must not contain @context",
            ));
            continue;
        }

        if is_ld_json && !has_context {
            parse_errors.push(BatchEntityError::new(
                entity_id,
                NgsiLdErrorType::BadRequestData,
                "application/ld+json body must contain @context",
            ));
            continue;
        }

        // Strip @context before deserializing
        if let Some(obj) = item.as_object_mut() {
            obj.remove("@context");
        }

        match serde_json::from_value::<Entity>(item) {
            Ok(entity) => entities.push(entity),
            Err(e) => {
                parse_errors.push(BatchEntityError::new(
                    entity_id,
                    NgsiLdErrorType::BadRequestData,
                    &format!("Invalid entity: {}", e),
                ));
            }
        }
    }

    Ok((entities, merged_context, parse_errors))
}

#[cfg(test)]
mod tests {
    use crate::{context::ContextUrl, entity_batch::parse_batch_entities};
    use andromeda_jsonld::context_spec::ContextSpec;
    use axum::{
        body::Bytes,
        http::{HeaderMap, HeaderValue, Uri, header::CONTENT_TYPE},
    };

    fn json_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    #[test]
    fn a_malformed_attribute_key_rejects_only_its_own_entity() {
        // NGSI-LD clause 5.6.7: batch creation reports failures per entity, so
        // one entity whose attribute key is not a valid NGSI-LD name must not
        // take the rest of the batch with it.
        let body = Bytes::from(
            serde_json::to_vec(&serde_json::json!([
                {"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": 10}},
                {"id": "urn:ngsi-ld:Vehicle:2", "type": "Vehicle", "bad name": {"type": "Property", "value": 20}},
                {"id": "urn:ngsi-ld:Vehicle:3", "type": "Vehicle", "speed": {"type": "Property", "value": 30}}
            ]))
            .expect("serialises"),
        );
        let uri = Uri::from_static("/ngsi-ld/v1/entityOperations/create");
        let (entities, _context, errors) = parse_batch_entities(&body, &json_headers(), &ContextUrl(ContextSpec::none()), &uri).expect("batch parses");

        pretty_assertions::assert_eq!(entities.len(), 2);
        pretty_assertions::assert_eq!(
            entities.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:1".to_string(), "urn:ngsi-ld:Vehicle:3".to_string()]
        );
        pretty_assertions::assert_eq!(errors.len(), 1);
        pretty_assertions::assert_eq!(errors[0].entity_id.as_str(), "urn:ngsi-ld:Vehicle:2");
    }
}
