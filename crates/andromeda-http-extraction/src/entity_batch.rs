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
use andromeda_ngsi_ld_number::json_value::{canonicalize_numbers, deserialize_exact};
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
use std::{collections::HashSet, mem::take};

/// Construct a deliberate public batch-extraction failure with request instance.
fn public_error(error_type: NgsiLdErrorType, detail: impl Into<String>, instance: &Uri) -> ProblemResponse {
    ClassifiedReport::public(error_type, detail).with_instance(instance.clone())
}

/// Parse a batch request body without narrowing arbitrary-precision numbers.
fn parse_batch_body_json(body: &Bytes) -> Result<Value, serde_json::Error> {
    serde_json::from_slice::<Value>(body)
}

/// Content-Type classification for batch entity parsing.
#[derive(Clone, Copy)]
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
        let headers = take(&mut parts.headers);
        let req = Request::from_parts(parts, body);
        let body = Bytes::from_request(req, state).await.map_err(IntoResponse::into_response)?;
        Ok(BatchBody { headers, body })
    }
}

/// Return whether any `Link` header names the JSON-LD context relation.
fn carries_context_link(headers: &HeaderMap) -> bool {
    headers
        .get_all("Link")
        .iter()
        .any(|v| v.to_str().is_ok_and(|s| s.contains("http://www.w3.org/ns/json-ld#context")))
}

/// Classify the batch representation the request announces.
///
/// Clause 6.3.5: an `application/ld+json` body names its context in the body,
/// so it must not be combined with a context `Link` header.
fn batch_content_kind(headers: &HeaderMap, uri: &Uri) -> Result<ContentKind, ProblemResponse> {
    let content_type_raw = headers.typed_get::<headers::ContentType>().map(|value| value.to_string()).unwrap_or_default();

    let content_kind = classify_batch_content_type(&content_type_raw)
        .ok_or_else(|| public_error(NgsiLdErrorType::InvalidRequest, format!("Unsupported Media Type: {content_type_raw}"), uri))?;

    match content_kind {
        ContentKind::LdJson if carries_context_link(headers) => Err(public_error(
            NgsiLdErrorType::InvalidRequest,
            "application/ld+json must not be combined with a Link context header",
            uri,
        )),
        ContentKind::Json | ContentKind::LdJson => Ok(content_kind),
    }
}

/// Decode the request body into the non-empty JSON array of entity
/// representations that clauses 5.6.7 to 5.6.10 require of a batch operation.
fn batch_items(body: &Bytes, uri: &Uri) -> Result<Vec<Value>, ProblemResponse> {
    if body.is_empty() {
        return Err(public_error(NgsiLdErrorType::InvalidRequest, "Request body shall not be empty", uri));
    }

    let value: Value = parse_batch_body_json(body).map_err(|error| public_error(NgsiLdErrorType::InvalidRequest, format!("Invalid JSON: {error}"), uri))?;

    match value {
        Value::Array(items) if items.is_empty() => Err(public_error(
            NgsiLdErrorType::BadRequestData,
            "Request body must contain a non-empty array of entities",
            uri,
        )),
        Value::Array(items) => Ok(items),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => Err(public_error(
            NgsiLdErrorType::BadRequestData,
            "Request body must be a JSON array of entities",
            uri,
        )),
    }
}

/// Fold the `@context` of every batch item into the request's context.
///
/// A batch is one request with one context, so the item contexts are merged
/// into a single body context, each distinct value kept once.
fn merge_item_contexts(items: &[Value], context: ContextSpec) -> ContextSpec {
    let mut seen = HashSet::new();
    let mut merged = Vec::new();

    for item in items {
        if let Some(ctx) = item.as_object().and_then(|o| o.get("@context")) {
            match ctx {
                Value::Array(entries) => {
                    for entry in entries {
                        let key = entry.to_string();
                        if seen.insert(key) {
                            merged.push(entry.clone()); // clone: collecting values from borrowed JSON
                        }
                    }
                }
                other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => {
                    let key = other.to_string();
                    if seen.insert(key) {
                        merged.push(other.clone()); // clone: collecting values from borrowed JSON
                    }
                }
            }
        }
    }

    let body_context = match merged.len() {
        0 => return context,
        1 => merged.into_iter().next(),
        _ => Some(Value::Array(merged)),
    };

    context.with_body_context(body_context)
}

/// The identifier a batch item must carry before anything can be reported
/// against it.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] for the malformed content that fails the
/// whole request: a null array item (clauses 5.6.7 to 5.6.10), or an item
/// whose `id` is absent or not a URI.
fn batch_entity_id(item: &Value, uri: &Uri) -> Result<EntityId, ProblemResponse> {
    // Clause 5.6.7 to 5.6.10: a null array item is malformed batch content,
    // reported as BadRequestData for the whole request rather than the
    // InvalidRequest a missing id raises.
    if item.is_null() {
        return Err(public_error(NgsiLdErrorType::BadRequestData, "Batch array items must not be null", uri));
    }

    item.as_object()
        .and_then(|o| o.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| public_error(NgsiLdErrorType::InvalidRequest, "Every batch entity must contain a valid URI id", uri))?
        .parse::<EntityId>()
        .map_err(|_error| public_error(NgsiLdErrorType::InvalidRequest, "Every batch entity must contain a valid URI id", uri))
}

/// Decode one batch array item into its entity.
///
/// # Errors
///
/// Returns a [`BatchEntityError`] against `entity_id` when the item carries a
/// number the model cannot represent, carries an `@context` its representation
/// forbids or lacks one it requires (clause 6.3.5), or is not an entity.
/// Clause 5.6.7 reports these per entity, leaving the rest of the batch intact.
fn decode_batch_item(mut item: Value, entity_id: EntityId, content_kind: ContentKind) -> Result<Entity, BatchEntityError> {
    if let Err(error) = canonicalize_numbers(&mut item) {
        return Err(BatchEntityError::new(entity_id, NgsiLdErrorType::BadRequestData, &error.to_string()));
    }

    let has_context = item.as_object().is_some_and(|o| o.contains_key("@context"));
    match content_kind {
        ContentKind::Json if has_context => {
            return Err(BatchEntityError::new(
                entity_id,
                NgsiLdErrorType::BadRequestData,
                "application/json body must not contain @context",
            ));
        }
        ContentKind::LdJson if !has_context => {
            return Err(BatchEntityError::new(
                entity_id,
                NgsiLdErrorType::BadRequestData,
                "application/ld+json body must contain @context",
            ));
        }
        ContentKind::Json | ContentKind::LdJson => {}
    }

    // Strip @context before deserializing: the context is carried separately.
    if let Some(obj) = item.as_object_mut() {
        obj.remove("@context");
    }

    deserialize_exact::<Entity>(&item).map_err(|error| BatchEntityError::new(entity_id, NgsiLdErrorType::BadRequestData, &format!("Invalid entity: {error}")))
}

/// Parse a batch request body with per-entity error handling.
///
/// Returns `(valid_entities, context, per_entity_errors)`.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the request fails as a whole: an
/// unsupported `Content-Type`, an `application/ld+json` body combined with a
/// context `Link` header (clause 6.3.5), an empty or unparsable body, a body
/// that is not a non-empty JSON array, a null array item, or an item without a
/// valid URI `id`. Every other failure is reported per entity in the returned
/// [`BatchEntityError`] list, leaving the rest of the batch intact.
pub fn parse_batch_entities(
    body: &Bytes,
    headers: &HeaderMap,
    context: &ContextUrl,
    uri: &Uri,
) -> Result<(Vec<Entity>, ContextSpec, Vec<BatchEntityError>), ProblemResponse> {
    let content_kind = batch_content_kind(headers, uri)?;
    let items = batch_items(body, uri)?;

    let request_context = context.0.clone(); // clone: need owned ContextSpec to merge body contexts into
    let merged_context = match content_kind {
        ContentKind::Json => request_context,
        ContentKind::LdJson => merge_item_contexts(&items, request_context),
    };

    let mut entities = Vec::new();
    let mut parse_errors = Vec::new();

    for item in items {
        let entity_id = batch_entity_id(&item, uri)?;
        match decode_batch_item(item, entity_id, content_kind) {
            Ok(entity) => entities.push(entity),
            Err(error) => parse_errors.push(error),
        }
    }

    Ok((entities, merged_context, parse_errors))
}

#[cfg(test)]
mod tests {
    use crate::{context::ContextUrl, entity_batch::parse_batch_entities};
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
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

        assert_eq!(entities.len(), 2, "{errors:?}");
        pretty_assertions::assert_eq!(
            entities.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:1".to_string(), "urn:ngsi-ld:Vehicle:3".to_string()]
        );
        pretty_assertions::assert_eq!(errors.len(), 1);
        pretty_assertions::assert_eq!(errors[0].entity_id.as_str(), "urn:ngsi-ld:Vehicle:2");
    }

    #[test]
    fn a_null_batch_array_item_is_bad_request_data() {
        // ETSI GS CIM 009 v1.9.1 clauses 5.6.7 to 5.6.10: a null array item is
        // malformed batch content and fails the whole request as BadRequestData,
        // not the InvalidRequest a missing id raises.
        let body = Bytes::from(serde_json::to_vec(&serde_json::json!([{"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"}, null])).expect("serialises"));
        let uri = Uri::from_static("/ngsi-ld/v1/entityOperations/create");
        let error = parse_batch_entities(&body, &json_headers(), &ContextUrl(ContextSpec::none()), &uri).expect_err("a null item fails the request");
        pretty_assertions::assert_eq!(error.problem().error_type(), NgsiLdErrorType::BadRequestData);
    }

    #[test]
    fn exact_numbers_survive_and_range_failures_remain_per_entity() {
        let body = Bytes::from(
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":0.100000000000000005}},{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle","json":{"type":"JsonProperty","json":{"nested":1e-16384}}},{"id":"urn:ngsi-ld:Vehicle:3","type":"Vehicle","speed":{"type":"Property","value":18446744073709551616}}]"#,
        );
        let uri = Uri::from_static("/ngsi-ld/v1/entityOperations/create");
        let (entities, _context, errors) = parse_batch_entities(&body, &json_headers(), &ContextUrl(ContextSpec::none()), &uri).expect("batch parses");

        assert_eq!(entities.len(), 2, "{errors:?}");
        let encoded = serde_json::to_string(&entities).expect("exact entities");
        assert!(encoded.contains("0.100000000000000005"), "{encoded}");
        assert!(encoded.contains("18446744073709551616"), "{encoded}");
        pretty_assertions::assert_eq!(errors.len(), 1);
        pretty_assertions::assert_eq!(errors[0].entity_id.as_str(), "urn:ngsi-ld:Vehicle:2");
        assert!(errors[0].error.detail.contains("/json/json/nested"), "{}", errors[0].error.detail);
    }
}
