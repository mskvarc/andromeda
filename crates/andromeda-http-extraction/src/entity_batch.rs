//! Batch entity request parsing for NGSI-LD batch operations.
//!
//! Provides [`BatchBody`] -- a [`FromRequest`] extractor that applies the clause
//! 6.3.4 request preconditions and classifies the representation the request
//! announced -- and [`parse_batch_entities`], which decodes the array a
//! [`BatchBody`] carries. Every Content-Type decision is taken once, in the
//! extractor: a `BatchBody` exists only for a request whose media type clause
//! 6.3.4 admits, so nothing below it has to re-derive that or answer for it.

use crate::{
    content_length_precondition::{ContentLengthPreconditionError, declared_body_length},
    context::ContextUrl,
};
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri as EntityId;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_number::json_value::{canonicalize_numbers, deserialize_exact};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchEntityError, error_type::NgsiLdErrorType};
use axum::{
    body::Bytes,
    extract::{FromRequest, FromRequestParts, OriginalUri, Request},
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

/// The representation a batch request announced in its `Content-Type`.
///
/// Clause 6.3.4 admits exactly these two for a body-taking operation, so the
/// classification is total once the extractor has admitted the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
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

/// The representation the request's `Content-Type` announces, if clause 6.3.4
/// admits it for a body-taking operation.
fn batch_content_type(headers: &HeaderMap) -> Option<ContentKind> {
    classify_batch_content_type(&headers.typed_get::<headers::ContentType>()?.to_string())
}

/// A batch request body that has passed the clause 6.3.4 preconditions.
///
/// The type is the evidence: it exists only for a request that declared a
/// payload length, announced a media type clause 6.3.4 admits, and did not
/// combine `application/ld+json` with a context `Link` header. Its fields are
/// private for that reason -- one cannot be assembled around a request that
/// failed any of those, so [`parse_batch_entities`] never has to ask again.
pub struct BatchBody {
    /// Request headers, replayed onto forwarded copies of the operation.
    headers: HeaderMap,
    /// The representation the request announced, classified once here.
    content_kind: ContentKind,
    /// Raw request body bytes.
    body: Bytes,
}

impl BatchBody {
    /// The request's own headers, which a forwarded copy of the operation
    /// replays (clause 6.3.19).
    #[must_use]
    pub const fn headers(&self) -> &HeaderMap {
        &self.headers
    }
}

impl<S> FromRequest<S> for BatchBody
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<BatchBody, Self::Rejection> {
        let (mut parts, body) = req.into_parts();
        // OriginalUri is infallible, so unwrap via match. It names the request
        // as the client addressed it, before nesting rewrote the path.
        let OriginalUri(uri) = match OriginalUri::from_request_parts(&mut parts, state).await {
            Ok(original) => original,
            Err(never) => match never {},
        };
        // Clause 6.3.4 checks both preconditions on a POST, and answers an
        // absent Content-Length with a bare 411 before the Content-Type is
        // looked at: a request that declares no payload length has declared no
        // payload for the media type to describe.
        declared_body_length(&parts.headers).map_err(|error| error.into_rejection(uri.clone()).into_response())?; // clone: the media-type refusal below names the same request
        // Clause 6.3.4: "Unsupported Media Type, i.e. 'Content-Type' header is
        // not 'application/json' nor 'application/ld+json', shall result in
        // just a 415 HTTP status code (without any payload body)". This is the
        // only place a batch request's media type is classified.
        let Some(content_kind) = batch_content_type(&parts.headers) else {
            return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response());
        };
        // Clause 6.3.5: an `application/ld+json` body names its context in the
        // body, so it must not be combined with a context `Link` header.
        if content_kind == ContentKind::LdJson && carries_context_link(&parts.headers) {
            return Err(public_error(
                NgsiLdErrorType::InvalidRequest,
                "application/ld+json must not be combined with a Link context header",
                &uri,
            )
            .into_response());
        }
        // Move headers out before reassembling the request for body extraction.
        // Bytes::from_request does not inspect headers, so empty headers are safe.
        let headers = take(&mut parts.headers);
        let req = Request::from_parts(parts, body);
        let body = Bytes::from_request(req, state).await.map_err(IntoResponse::into_response)?;
        // A declared length the payload did not carry leaves clause 5.5.4 with
        // no JSON document to read, which is the same refusal a declared length
        // of zero draws.
        if body.is_empty() {
            return Err(ContentLengthPreconditionError::Empty.into_rejection(uri).into_response());
        }
        Ok(BatchBody { headers, content_kind, body })
    }
}

/// Return whether any `Link` header names the JSON-LD context relation.
fn carries_context_link(headers: &HeaderMap) -> bool {
    headers
        .get_all("Link")
        .iter()
        .any(|v| v.to_str().is_ok_and(|s| s.contains("http://www.w3.org/ns/json-ld#context")))
}

/// Decode the request body into the non-empty JSON array of entity
/// representations that clauses 5.6.7 to 5.6.10 require of a batch operation.
///
/// The body is known to be non-empty: [`BatchBody`] exists only for a request
/// that declared a payload length and carried one.
fn batch_items(body: &Bytes, uri: &Uri) -> Result<Vec<Value>, ProblemResponse> {
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
/// The per-entity failure is boxed: it is three times the size of the decoded
/// entity, and the batch is dominated by items that decode.
///
/// # Errors
///
/// Returns a [`BatchEntityError`] against `entity_id` when the item carries a
/// number the model cannot represent, carries an `@context` its representation
/// forbids or lacks one it requires (clause 6.3.5), or is not an entity.
/// Clause 5.6.7 reports these per entity, leaving the rest of the batch intact.
fn decode_batch_item(mut item: Value, entity_id: EntityId, content_kind: ContentKind) -> Result<Entity, Box<BatchEntityError>> {
    if let Err(error) = canonicalize_numbers(&mut item) {
        return Err(Box::new(BatchEntityError::new(entity_id, NgsiLdErrorType::BadRequestData, &error.to_string())));
    }

    let has_context = item.as_object().is_some_and(|o| o.contains_key("@context"));
    match content_kind {
        ContentKind::Json if has_context => {
            return Err(Box::new(BatchEntityError::new(
                entity_id,
                NgsiLdErrorType::BadRequestData,
                "application/json body must not contain @context",
            )));
        }
        ContentKind::LdJson if !has_context => {
            return Err(Box::new(BatchEntityError::new(
                entity_id,
                NgsiLdErrorType::BadRequestData,
                "application/ld+json body must contain @context",
            )));
        }
        ContentKind::Json | ContentKind::LdJson => {}
    }

    // Strip @context before deserializing: the context is carried separately.
    if let Some(obj) = item.as_object_mut() {
        obj.remove("@context");
    }

    deserialize_exact::<Entity>(&item).map_err(|error| {
        Box::new(BatchEntityError::new(
            entity_id,
            NgsiLdErrorType::BadRequestData,
            &format!("Invalid entity: {error}"),
        ))
    })
}

/// Parse a batch request body with per-entity error handling.
///
/// Returns `(valid_entities, context, per_entity_errors)`.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the request fails as a whole: an
/// unparsable body, a body that is not a non-empty JSON array, a null array
/// item, or an item without a valid URI `id`. Every other failure is reported
/// per entity in the returned [`BatchEntityError`] list, leaving the rest of
/// the batch intact. The clause 6.3.4 media-type and payload-length refusals
/// are not among them: [`BatchBody`] raised those before this was reached.
pub fn parse_batch_entities(batch: &BatchBody, context: &ContextUrl, uri: &Uri) -> Result<(Vec<Entity>, ContextSpec, Vec<BatchEntityError>), ProblemResponse> {
    let content_kind = batch.content_kind;
    let items = batch_items(&batch.body, uri)?;

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
            Err(error) => parse_errors.push(*error),
        }
    }

    Ok((entities, merged_context, parse_errors))
}

#[cfg(test)]
mod tests {
    use crate::{
        context::ContextUrl,
        entity_batch::{BatchBody, parse_batch_entities},
    };
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::{
        body::{Body, to_bytes},
        extract::FromRequest,
        http::{
            Request,
            StatusCode,
            Uri,
            header::{CONTENT_LENGTH, CONTENT_TYPE},
        },
    };

    /// The URI every fixture request addresses.
    fn batch_uri() -> Uri {
        Uri::from_static("/ngsi-ld/v1/entityOperations/create")
    }

    /// The batch body an `application/json` request carries, as the extractor
    /// admits it: the clause 6.3.4 preconditions are the extractor's, so a
    /// fixture reaches [`parse_batch_entities`] only by passing them.
    async fn json_batch(body: &'static str) -> BatchBody {
        let request = Request::builder()
            .uri(batch_uri())
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, body.len())
            .body(Body::from(body))
            .expect("request builds");
        match BatchBody::from_request(request, &()).await {
            Ok(batch) => batch,
            Err(refusal) => panic!("the fixture meets the clause 6.3.4 preconditions, got {}", refusal.status()),
        }
    }

    #[tokio::test]
    async fn a_malformed_attribute_key_rejects_only_its_own_entity() {
        // NGSI-LD clause 5.6.7: batch creation reports failures per entity, so
        // one entity whose attribute key is not a valid NGSI-LD name must not
        // take the rest of the batch with it.
        let batch = json_batch(
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":10}},{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle","bad name":{"type":"Property","value":20}},{"id":"urn:ngsi-ld:Vehicle:3","type":"Vehicle","speed":{"type":"Property","value":30}}]"#,
        )
        .await;
        let (entities, _context, errors) = parse_batch_entities(&batch, &ContextUrl(ContextSpec::none()), &batch_uri()).expect("batch parses");

        assert_eq!(entities.len(), 2, "{errors:?}");
        pretty_assertions::assert_eq!(
            entities.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:1".to_string(), "urn:ngsi-ld:Vehicle:3".to_string()]
        );
        pretty_assertions::assert_eq!(errors.len(), 1);
        pretty_assertions::assert_eq!(errors[0].entity_id.as_str(), "urn:ngsi-ld:Vehicle:2");
    }

    #[tokio::test]
    async fn a_null_batch_array_item_is_bad_request_data() {
        // ETSI GS CIM 009 v1.9.1 clauses 5.6.7 to 5.6.10: a null array item is
        // malformed batch content and fails the whole request as BadRequestData,
        // not the InvalidRequest a missing id raises.
        let batch = json_batch(r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle"},null]"#).await;
        let error = parse_batch_entities(&batch, &ContextUrl(ContextSpec::none()), &batch_uri()).expect_err("a null item fails the request");
        pretty_assertions::assert_eq!(error.problem().error_type(), NgsiLdErrorType::BadRequestData);
    }

    #[tokio::test]
    async fn exact_numbers_survive_and_range_failures_remain_per_entity() {
        let batch = json_batch(
            r#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":0.100000000000000005}},{"id":"urn:ngsi-ld:Vehicle:2","type":"Vehicle","json":{"type":"JsonProperty","json":{"nested":1e-16384}}},{"id":"urn:ngsi-ld:Vehicle:3","type":"Vehicle","speed":{"type":"Property","value":18446744073709551616}}]"#,
        )
        .await;
        let (entities, _context, errors) = parse_batch_entities(&batch, &ContextUrl(ContextSpec::none()), &batch_uri()).expect("batch parses");

        assert_eq!(entities.len(), 2, "{errors:?}");
        let encoded = serde_json::to_string(&entities).expect("exact entities");
        assert!(encoded.contains("0.100000000000000005"), "{encoded}");
        assert!(encoded.contains("18446744073709551616"), "{encoded}");
        pretty_assertions::assert_eq!(errors.len(), 1);
        pretty_assertions::assert_eq!(errors[0].entity_id.as_str(), "urn:ngsi-ld:Vehicle:2");
        assert!(errors[0].error.detail.contains("/json/json/nested"), "{}", errors[0].error.detail);
    }

    /// Clause 6.3.4: "Unsupported Media Type, i.e. 'Content-Type' header is not
    /// 'application/json' nor 'application/ld+json', shall result in just a 415
    /// HTTP status code (without any payload body)". The media type is decided
    /// once, here, so nothing below can answer it with a problem detail.
    #[tokio::test]
    async fn an_unsupported_media_type_is_the_bare_415_of_clause_6_3_4() {
        let request = Request::builder()
            .uri(batch_uri())
            .header(CONTENT_TYPE, "application/xml")
            .header(CONTENT_LENGTH, 8)
            .body(Body::from("<batch/>"))
            .expect("request builds");

        let (status, body) = batch_refusal(request).await;

        pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert!(body.is_empty(), "clause 6.3.4 forbids a payload body on a 415: {body:?}");
    }

    /// The status and body a batch request is refused with before its payload
    /// is read.
    async fn batch_refusal(request: Request<Body>) -> (StatusCode, Vec<u8>) {
        let response = BatchBody::from_request(request, &()).await.err().expect("the request is refused");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap_or_default();
        (status, body.to_vec())
    }

    /// Clause 6.3.4: "'Content-Length' HTTP header absence, shall result in
    /// just a 411 HTTP status code (without any payload body)". A batch
    /// operation is an HTTP POST, so the precondition applies to it exactly as
    /// it does to a single-entity POST.
    #[tokio::test]
    async fn a_batch_request_declaring_no_length_is_the_bare_411_of_clause_6_3_4() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/create")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from("[]"))
            .expect("request builds");

        let (status, body) = batch_refusal(request).await;

        pretty_assertions::assert_eq!(status, StatusCode::LENGTH_REQUIRED);
        assert!(body.is_empty(), "clause 6.3.4 forbids a payload body on a 411: {body:?}");
    }

    /// Clause 6.3.4 raises the 411 before the `Content-Type` precondition, so a
    /// request that meets neither is answered by the length it did not declare.
    #[tokio::test]
    async fn an_unsupported_media_type_without_a_length_is_still_the_411() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/create")
            .header(CONTENT_TYPE, "application/xml")
            .body(Body::from("<batch/>"))
            .expect("request builds");

        pretty_assertions::assert_eq!(batch_refusal(request).await.0, StatusCode::LENGTH_REQUIRED);
    }

    /// Clause 6.3.4: an unsupported `Content-Type` "shall result in just a 415
    /// HTTP status code (without any payload body)" once the length is declared.
    #[tokio::test]
    async fn a_declared_length_leaves_the_unsupported_media_type_a_bare_415() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/create")
            .header(CONTENT_TYPE, "application/xml")
            .header(CONTENT_LENGTH, "8")
            .body(Body::from("<batch/>"))
            .expect("request builds");

        let (status, body) = batch_refusal(request).await;

        pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert!(body.is_empty(), "clause 6.3.4 forbids a payload body on a 415: {body:?}");
    }

    /// Clause 5.5.4: a declared zero-length payload body is not a valid JSON
    /// document, so it is `InvalidRequest` rather than the 411 of an absent
    /// header.
    #[tokio::test]
    async fn a_batch_request_declaring_a_zero_length_is_an_invalid_request() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/create")
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, "0")
            .body(Body::empty())
            .expect("request builds");

        let (status, body) = batch_refusal(request).await;

        pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
        let problem: serde_json::Value = serde_json::from_slice(&body).expect("a problem details body");
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest");
    }

    /// A batch request that meets both preconditions reaches its payload.
    #[tokio::test]
    async fn a_batch_request_meeting_the_preconditions_is_extracted() {
        let request = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/create")
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, "2")
            .body(Body::from("[]"))
            .expect("request builds");

        let extracted = BatchBody::from_request(request, &()).await.expect("both preconditions are met");

        pretty_assertions::assert_eq!(extracted.body.as_ref(), b"[]".as_slice());
    }
}
