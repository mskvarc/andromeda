//! JSON-LD body extractor with context extraction.
//!
//! Uses `mediatype` for Content-Type validation instead of substring matching,
//! preventing false positives (e.g. `application/json-patch+json` no longer
//! matches `application/json`).

use crate::{
    addressed_uri::addressed_request_uri,
    content_length_precondition::{ContentLengthPreconditionError, declared_body_length},
    request_payload::buffer_payload,
    undecodable_body::UndecodableBody,
};
use andromeda_http_headers::link::header::Link;
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_number::json_value::{canonicalize_numbers, deserialize_exact, deserialize_exact_document};
use axum::{
    extract::{FromRequest, Request},
    http::{StatusCode, Uri, header::LINK},
};
use headers::HeaderMapExt;
use mediatype::{MediaTypeBuf, names};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{collections::HashSet, str::FromStr};

/// Axum extractor for NGSI-LD JSON-LD request bodies decoded into a domain type.
///
/// Validates the `Content-Type` header against the supported NGSI-LD media types
/// (`application/json`, `application/ld+json`, `application/merge-patch+json`),
/// extracts and merges `@context` values from the body when present, and
/// deserializes the context-stripped payload into `T`. A handler that keeps
/// the body as an untyped JSON document takes [`JsonLdDocument`] instead.
pub struct JsonLd<T> {
    /// The deserialized request body with `@context` removed.
    pub value: T,
    /// The extracted context specification from the body and/or Link header.
    pub context: ContextSpec,
}

/// Axum extractor for NGSI-LD JSON-LD request bodies kept as untyped JSON
/// documents.
///
/// Applies every precondition [`JsonLd`] applies and yields the same document
/// `JsonLd<Value>` would, without re-reading a document that is
/// already in the shape a read of its serialization gives.
pub struct JsonLdDocument {
    /// The request body with `@context` removed.
    pub document: Value,
    /// The extracted context specification from the body and/or Link header.
    pub context: ContextSpec,
}

/// A request body that passed the NGSI-LD body preconditions, with its
/// numbers canonicalized and its `@context` split off.
struct AdmittedBody {
    /// The context-stripped body.
    document: Value,
    /// The extracted context specification from the body and/or Link header.
    context: ContextSpec,
    /// The request URI, the `instance` of any later rejection.
    uri: Uri,
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

/// Classify a raw content type into a supported JSON representation kind.
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

/// Return whether a decoded document carries a top-level JSON-LD context.
fn document_has_context(document: &Value) -> bool {
    match document {
        Value::Object(obj) => obj.contains_key("@context"),
        Value::Array(items) => items.iter().any(|item| item.as_object().is_some_and(|o| o.contains_key("@context"))),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

/// Merge the context values carried by a document into one JSON value.
fn merge_context_values_from_document(document: &Value) -> Option<Value> {
    let mut merged = Vec::new();
    let mut seen = HashSet::new();

    let mut push_context = |ctx: Value| match ctx {
        Value::Array(items) => {
            for item in items {
                let key = item.to_string();
                if seen.insert(key) {
                    merged.push(item);
                }
            }
        }
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => {
            let key = other.to_string();
            if seen.insert(key) {
                merged.push(other);
            }
        }
    };

    match document {
        Value::Object(obj) => {
            if let Some(ctx) = obj.get("@context") {
                push_context(ctx.clone());
            }
        }
        Value::Array(items) => {
            for item in items {
                if let Some(ctx) = item.as_object().and_then(|obj| obj.get("@context")) {
                    push_context(ctx.clone());
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }

    match merged.len() {
        0 => None,
        1 => merged.into_iter().next(),
        _ => Some(Value::Array(merged)),
    }
}

/// Whether a `@context` value has one of the shapes JSON-LD 1.1 section 3.1
/// gives it: an IRI string, a context definition object, or an array of those
/// (and `null`, which resets). Clause 5.5.4: any other shape is data that is
/// not syntactically correct according to the API data type definitions.
fn is_well_formed_context(context: &Value) -> bool {
    match context {
        Value::Null | Value::String(_) | Value::Object(_) => true,
        Value::Array(items) => items.iter().all(|item| matches!(item, Value::Null | Value::String(_) | Value::Object(_))),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

/// Whether every top-level `@context` a document carries is well formed.
fn document_contexts_are_well_formed(document: &Value) -> bool {
    match document {
        Value::Object(obj) => obj.get("@context").is_none_or(is_well_formed_context),
        Value::Array(items) => items
            .iter()
            .all(|item| item.as_object().and_then(|o| o.get("@context")).is_none_or(is_well_formed_context)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
    }
}

/// Remove top-level JSON-LD context members before policy deserialization.
fn strip_context_from_value(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            obj.remove("@context");
        }
        Value::Array(items) => {
            for item in items {
                if let Value::Object(obj) = item {
                    obj.remove("@context");
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

/// Reject a body that does not decode into the extractor's target.
fn undecodable_body(error: &UndecodableBody, uri: Uri) -> ExtractorRejection {
    ExtractorRejection::bad_request_data(error.to_string(), uri)
}

impl<S, T> FromRequest<S> for JsonLd<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ExtractorRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let AdmittedBody { document, context, uri } = admit_body(req, state).await?;
        let value = deserialize_exact(&document).map_err(|source| undecodable_body(&UndecodableBody::of::<T>(&document, source), uri))?;
        Ok(JsonLd { value, context })
    }
}

impl<S> FromRequest<S> for JsonLdDocument
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let AdmittedBody { document, context, uri } = admit_body(req, state).await?;
        let document = deserialize_exact_document(document).map_err(|source| undecodable_body(&UndecodableBody::Document { source }, uri))?;
        Ok(JsonLdDocument { document, context })
    }
}

/// Apply the clause 6.3.4 and 6.3.5 request preconditions, parse the body,
/// canonicalize its numbers, and split its `@context` off.
async fn admit_body<S: Send + Sync>(req: Request, state: &S) -> Result<AdmittedBody, ExtractorRejection> {
    let uri = addressed_request_uri(&req);
    // Clause 6.3.4: the payload length is checked before the payload is
    // looked at, so an absent Content-Length is the bare 411 the clause
    // requires rather than a decoding failure.
    let declared = declared_body_length(req.headers()).map_err(|error| error.into_rejection(uri.clone()))?;
    let content_type_raw = req
        .headers()
        .typed_get::<headers::ContentType>()
        .map(|value| value.to_string())
        .unwrap_or_default();

    let Some(content_kind) = classify_content_type(&content_type_raw) else {
        tracing::warn!("JsonLd extraction failed: unsupported media type '{}'", content_type_raw);
        return Err(ExtractorRejection::Empty(StatusCode::UNSUPPORTED_MEDIA_TYPE));
    };

    let is_ld_json = matches!(content_kind, ContentKind::LdJson);
    let is_plain_json = matches!(content_kind, ContentKind::Json);

    // Collect raw Link header strings before the request body is consumed.
    let link_header_strings: Vec<String> = req.headers().get_all(LINK).iter().filter_map(|v| v.to_str().ok().map(String::from)).collect();
    let mut link_contexts = Vec::new();
    for raw in &link_header_strings {
        let parsed = Link::from_str(raw).map_err(|_| ExtractorRejection::bad_request_data("Invalid Link header", uri.clone()))?;
        link_contexts.extend(parsed.context_urls().cloned());
    }

    // Per NGSI-LD 6.3.5: application/ld+json MUST NOT be combined with a Link header
    // containing a JSON-LD context relation
    if is_ld_json {
        let has_context_link = link_header_strings.iter().any(|s| s.contains("http://www.w3.org/ns/json-ld#context"));
        if has_context_link {
            return Err(ExtractorRejection::bad_request_data(
                "application/ld+json must not be combined with a Link context header",
                uri,
            ));
        }
    }

    let bytes = buffer_payload(req, state, declared, uri.clone()).await?;

    // A declared length the payload did not carry leaves clause 5.5.4 with
    // no JSON document to read, which is the refusal the `Content-Length`
    // precondition already owns.
    if bytes.is_empty() {
        return Err(ContentLengthPreconditionError::Empty.into_rejection(uri.clone()));
    }

    let mut value: Value =
        serde_json::from_slice(&bytes).map_err(|error| ExtractorRejection::invalid_request(format!("Invalid JSON: {error}"), uri.clone()))?;
    canonicalize_numbers(&mut value).map_err(|error| ExtractorRejection::bad_request_data(error.to_string(), uri.clone()))?;

    // Reject empty arrays — not a valid NGSI-LD entity or batch payload
    if value.as_array().is_some_and(Vec::is_empty) {
        return Err(ExtractorRejection::bad_request_data(
            "Invalid JSON: expected object, got empty array",
            uri.clone(),
        ));
    }

    // Clause 6.3.21: a profile `Link` announcing the payload's NGSI-LD
    // version is read by the `PayloadProfile` extractor. This broker
    // interprets every version the clause 4.3.6.8 tables describe, so no
    // amendment of the payload is needed for it to be accepted here.
    // Per NGSI-LD 6.3.5: application/json MUST NOT contain @context in body
    if is_plain_json && document_has_context(&value) {
        return Err(ExtractorRejection::bad_request_data("application/json body must not contain @context", uri));
    }

    // Per NGSI-LD 6.3.5: application/ld+json MUST contain @context in body
    if is_ld_json && !document_has_context(&value) {
        return Err(ExtractorRejection::bad_request_data("application/ld+json body must contain @context", uri));
    }

    if !document_contexts_are_well_formed(&value) {
        return Err(ExtractorRejection::bad_request_data(
            "@context must be a string, an object, or an array of those",
            uri,
        ));
    }

    let mut context = ContextSpec::from_links(link_contexts);
    if is_ld_json && let Some(ctx) = merge_context_values_from_document(&value) {
        context = context.with_body_context(Some(ctx));
    }

    // Always strip @context before deserializing into Entity
    // Context is handled separately by ContextUrl extractor
    strip_context_from_value(&mut value);

    Ok(AdmittedBody { document: value, context, uri })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, HttpBody},
        http::{
            HeaderValue,
            Request,
            header::{CONTENT_LENGTH, CONTENT_TYPE},
        },
    };

    /// Declare the fixed length of a test request's body.
    fn with_content_length(mut request: Request<Body>) -> Request<Body> {
        let length = request.body().size_hint().exact().expect("a fixed test body length");
        let value = HeaderValue::from_str(&length.to_string()).expect("a valid content length");
        request.headers_mut().insert(CONTENT_LENGTH, value);
        request
    }

    async fn extract<T>(request: Request<Body>) -> Result<JsonLd<T>, ExtractorRejection>
    where
        T: DeserializeOwned,
    {
        JsonLd::<T>::from_request(with_content_length(request), &()).await
    }

    async fn extract_document(request: Request<Body>) -> Result<JsonLdDocument, ExtractorRejection> {
        JsonLdDocument::from_request(with_content_length(request), &()).await
    }

    /// A JSON request carrying `body`.
    fn json_request(body: &'static str) -> Request<Body> {
        Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .expect("request builds")
    }

    #[tokio::test]
    async fn rejects_unsupported_media_type() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/xml")
            .body(Body::from("{}"))
            .unwrap();

        let result = extract_document(req).await;
        match result {
            Err(ExtractorRejection::Empty(status)) => {
                pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
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
            .body(Body::from("{}"))
            .unwrap();

        let result = extract_document(req).await;
        match result {
            Err(ExtractorRejection::Empty(status)) => {
                pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
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
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let result = extract_document(req).await;
        assert!(result.is_ok(), "merge-patch+json should be accepted");
    }

    #[tokio::test]
    async fn rejects_empty_body_with_bad_request() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::empty())
            .unwrap();

        let result = extract_document(req).await;
        assert!(matches!(result, Err(ExtractorRejection::InvalidRequest { .. })));
    }

    #[tokio::test]
    async fn rejects_valid_empty_array_as_bad_request_data() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from("[]"))
            .unwrap();

        let result = extract_document(req).await;
        assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })));
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
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let result = extract_document(req).await.unwrap();
        pretty_assertions::assert_eq!(result.context.body_context(), Some(&serde_json::json!("http://example.org/context")));
        assert!(result.document.as_object().unwrap().get("@context").is_none());
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
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let result = extract_document(req).await.unwrap();
        pretty_assertions::assert_eq!(
            result.context.body_context(),
            Some(&serde_json::json!(["https://example.org/a", "https://example.org/b"]))
        );
    }

    #[tokio::test]
    async fn a_context_that_is_not_a_string_object_or_array_is_bad_request_data() {
        // Clause 5.5.4: a `@context` shaped outside JSON-LD section 3.1 is
        // syntactically incorrect data.
        for context in [
            serde_json::json!(42),
            serde_json::json!(true),
            serde_json::json!([1, "https://example.org/ctx"]),
        ] {
            let body = serde_json::json!({"@context": context, "id": "urn:ngsi-ld:T:1", "type": "T"});
            let req = Request::builder()
                .uri("/ngsi-ld/v1/entities")
                .header(CONTENT_TYPE, "application/ld+json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap();
            let result = extract_document(req).await;
            assert!(matches!(result, Err(ExtractorRejection::BadRequestData { .. })), "{context}");
        }
    }

    #[tokio::test]
    async fn content_type_with_charset_param_works() {
        let body = serde_json::json!({"id": "urn:ngsi-ld:T:1", "type": "T"});
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/json; charset=utf-8")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let result = extract_document(req).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn nested_numbers_are_preserved_without_a_binary64_hop() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"id":"urn:ngsi-ld:T:1","type":"T","property":{"type":"Property","value":0.100000000000000005,"nested":[18446744073709551616]},"list":{"type":"ListProperty","valueList":[-2.5e-400]},"json":{"type":"JsonProperty","json":{"n":1.234567890123456789e20}}}"#,
            ))
            .expect("request builds");

        let extracted = extract_document(req).await.expect("supported exact numbers");
        let encoded = serde_json::to_string(&extracted.document).expect("exact JSON");
        assert!(encoded.contains("0.100000000000000005"), "{encoded}");
        assert!(encoded.contains("18446744073709551616"), "{encoded}");
        assert!(encoded.contains("-2.5e-400") || encoded.contains("-2.5E-400"), "{encoded}");
        assert!(encoded.contains("123456789012345678900"), "{encoded}");
    }

    #[tokio::test]
    async fn an_out_of_range_nested_number_is_bad_request_data_with_its_path() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"id":"urn:ngsi-ld:T:1","type":"T","property":{"type":"Property","value":{"nested":[1e-16384]}}}"#,
            ))
            .expect("request builds");

        match extract_document(req).await {
            Err(ExtractorRejection::BadRequestData { detail, .. }) => {
                assert!(detail.contains("/property/value/nested/0"), "{detail}");
            }
            _other => panic!("expected BadRequestData"),
        }
    }

    /// Clause 6.3.4: "'Content-Length' HTTP header absence, shall result in
    /// just a 411 HTTP status code (without any payload body)", and it is
    /// checked before the `Content-Type` precondition, so a request that fails
    /// both is answered by the header it did not send.
    #[tokio::test]
    async fn a_request_declaring_no_length_is_the_bare_411_of_clause_6_3_4() {
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/xml")
            .body(Body::from("{}"))
            .expect("request builds");

        match JsonLdDocument::from_request(req, &()).await {
            Err(ExtractorRejection::Empty(status)) => pretty_assertions::assert_eq!(status, StatusCode::LENGTH_REQUIRED),
            _other => panic!("expected a bare 411"),
        }
    }

    #[tokio::test]
    async fn an_untyped_document_is_the_document_a_value_typed_extraction_gives() {
        let bodies = [
            r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":{"type":"Property","value":[21.5,-7,0,18446744073709551616,0.100000000000000005]}}"#,
            r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":{"type":"Property","value":[1e-7,-2.5e-400,1E+20,1e20,0.00001,1.50]}}"#,
            r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":{"type":"Property","value":{"b":1,"$serde_json::private::Number":"5"}}}"#,
            r#"[{"id":"urn:ngsi-ld:T:1","type":"T"},{"id":"urn:ngsi-ld:T:2","type":"T","s":"text"}]"#,
        ];
        for body in bodies {
            let untyped = extract_document(json_request(body)).await.map(|extracted| extracted.document.to_string());
            let typed = extract::<Value>(json_request(body)).await.map(|extracted| extracted.value.to_string());
            match (untyped, typed) {
                (Ok(untyped), Ok(typed)) => pretty_assertions::assert_eq!(untyped, typed, "{body}"),
                // Both refuse the document; the typed extraction can also name
                // the member it failed at, which an untyped one has none of.
                (Err(ExtractorRejection::BadRequestData { .. }), Err(ExtractorRejection::BadRequestData { .. })) => {}
                _other => panic!("{body}: the untyped and Value-typed extractions disagree"),
            }
        }
    }

    #[tokio::test]
    async fn a_typed_body_that_does_not_decode_is_bad_request_data_naming_the_member_and_the_positioned_reader_message() {
        #[derive(Debug, serde::Deserialize)]
        struct Named {
            name: String,
        }

        match extract::<Named>(json_request(r#"{"name":5}"#)).await {
            Err(ExtractorRejection::BadRequestData { detail, .. }) => {
                pretty_assertions::assert_eq!(
                    detail,
                    "the request body member 'name' does not decode: invalid type: integer `5`, expected a string at line 1 column 9"
                );
            }
            Ok(extracted) => panic!("accepted the name {}", extracted.value.name),
            Err(other) => panic!("expected BadRequestData, got {other:?}"),
        }
    }
}
