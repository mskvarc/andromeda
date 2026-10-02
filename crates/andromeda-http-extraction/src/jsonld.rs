//! JSON-LD body extractor with context extraction.
//!
//! Uses `mediatype` for Content-Type validation instead of substring matching,
//! preventing false positives (e.g. `application/json-patch+json` no longer
//! matches `application/json`).
//!
//! A body is admitted from its text alone: its JSON values are counted, its
//! numbers checked and its top-level `@context` read without building it, and
//! a [`JsonLd`] body is then decoded straight from the text into its domain
//! type. Only a [`JsonLdDocument`], which a handler keeps untyped, is parsed
//! into a JSON tree.

use crate::{
    addressed_uri::addressed_request_uri,
    admitted_document::admitted_document,
    blocking_decode::{BlockingDecodePolicy, decode_payload},
    body_envelope::BodyEnvelope,
    classification::classify_context_link_error,
    content_length_precondition::{ContentLengthPreconditionError, declared_body_length},
    json_value_budget::refuse_values_past_budget,
    request_json_value_limit::RequestJsonValueLimit,
    request_links::{PayloadCarrier, RequestLinks},
    request_payload::buffer_payload,
    undecodable_body::UndecodableBody,
};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_jsonld::{context_spec::ContextSpec, request_context::RequestContext};
use andromeda_ngsi_ld_model::request_body::RequestBody;
use andromeda_ngsi_ld_number::json_value::{
    deserialize_exact_document,
    number_range::{NumberRangeError, refuse_numbers_out_of_range},
};
use axum::{
    body::Bytes,
    extract::{FromRequest, Request},
    http::{StatusCode, Uri},
};
use headers::HeaderMapExt;
use iri_rs::IriBuf;
use mediatype::{MediaTypeBuf, names};
use serde_json::Value;

/// Axum extractor for NGSI-LD JSON-LD request bodies decoded into a domain type.
///
/// Validates the `Content-Type` header against the supported NGSI-LD media types
/// (`application/json`, `application/ld+json`, `application/merge-patch+json`),
/// extracts and merges `@context` values from the body when present, and
/// decodes the body, its `@context` member skipped, straight from its text into
/// `T`. A handler that keeps the body as an untyped JSON document takes
/// [`JsonLdDocument`] instead.
pub struct JsonLd<T> {
    /// The deserialized request body with `@context` removed.
    pub value: T,
    /// The extracted context specification from the body and/or Link header.
    pub context: ContextSpec,
}

/// Axum extractor for NGSI-LD JSON-LD request bodies kept as untyped JSON
/// documents.
///
/// Applies every precondition [`JsonLd`] applies and yields the document with
/// its numbers in canonical spelling and its `@context` removed, in the shape
/// a read of its serialization gives.
pub struct JsonLdDocument {
    /// The request body with `@context` removed.
    pub document: Value,
    /// The extracted context specification from the body and/or Link header.
    pub context: ContextSpec,
}

/// A request body that passed the NGSI-LD body preconditions, with its
/// `@context` read.
struct AdmittedBody {
    /// The payload as received.
    text: Bytes,
    /// The extracted context specification from the body and/or Link header.
    context: ContextSpec,
    /// The request URI, the `instance` of any later rejection.
    uri: Uri,
}

/// Classify the Content-Type into one of the supported NGSI-LD types.
#[derive(Clone, Copy)]
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

/// The refusal of a body whose JSON or numbers are refused: a text that is no
/// JSON is an `InvalidRequest`, a number the model cannot represent or a
/// member named like the reader's own tokens `BadRequestData` (clause 5.5.4).
fn number_rejection(refusal: NumberRangeError, uri: Uri) -> ExtractorRejection {
    match refusal {
        NumberRangeError::Syntax(error) => ExtractorRejection::invalid_request(format!("Invalid JSON: {error}"), uri),
        refused @ (NumberRangeError::OutOfRange(_) | NumberRangeError::ReservedMemberName { .. }) => {
            ExtractorRejection::bad_request_data(refused.to_string(), uri)
        }
    }
}

/// Reject a body that does not decode into the extractor's target.
fn undecodable_body(error: &UndecodableBody, uri: Uri) -> ExtractorRejection {
    ExtractorRejection::bad_request_data(error.to_string(), uri)
}

impl<S, T> FromRequest<S> for JsonLd<T>
where
    S: Send + Sync,
    T: RequestBody + Send + 'static,
{
    type Rejection = ExtractorRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        read_body(req, state)
            .await?
            .decode(|AdmittedBody { text, context, uri }| {
                let value = T::from_request_text(&text).map_err(|source| undecodable_body(&UndecodableBody::of_text::<T>(&text, source), uri))?;
                Ok(JsonLd { value, context })
            })
            .await
    }
}

impl<S> FromRequest<S> for JsonLdDocument
where
    S: Send + Sync,
{
    type Rejection = ExtractorRejection;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        read_body(req, state)
            .await?
            .decode(|AdmittedBody { text, context, uri }| {
                // clone: each refusal names the request
                let document = admitted_document(&text).map_err(|refusal| number_rejection(refusal, uri.clone()))?;
                let document = deserialize_exact_document(document).map_err(|source| undecodable_body(&UndecodableBody::Document { source }, uri))?;
                Ok(JsonLdDocument { document, context })
            })
            .await
    }
}

/// A request body that passed the clause 6.3.4 and 6.3.5 header
/// preconditions and was read in full, not yet parsed.
struct ReadBody {
    /// The payload as received.
    bytes: Bytes,
    /// The representation the request announced.
    content_kind: ContentKind,
    /// Where the payload's context is named.
    carrier: PayloadCarrier,
    /// The context `Link` the request carried, read before the body.
    link_context: Option<IriBuf>,
    /// The request URI, the `instance` of any later rejection.
    uri: Uri,
    /// The most JSON values the payload may hold, when the stack published it.
    value_limit: Option<RequestJsonValueLimit>,
    /// How the payload is decoded beside the executor, when the stack
    /// published it.
    blocking_decode: Option<BlockingDecodePolicy>,
}

impl ReadBody {
    /// Admit the payload, then hand the admitted body to `finish`, all of it
    /// on the blocking pool when the payload is large.
    ///
    /// # Errors
    ///
    /// Returns the refusal of [`ReadBody::admit`] or of `finish`, and an
    /// `InternalError` when the blocking task fails.
    async fn decode<T: Send + 'static>(
        self,
        finish: impl FnOnce(AdmittedBody) -> Result<T, ExtractorRejection> + Send + 'static,
    ) -> Result<T, ExtractorRejection> {
        let uri = self.uri.clone(); // clone: a failed decoding task still names the request
        let blocking_decode = self.blocking_decode.clone(); // clone: the decode moves the body it was read with
        decode_payload(blocking_decode.as_ref(), self.bytes.len(), move || self.admit().and_then(finish))
            .await
            .map_err(|failure| ExtractorRejection::Classified(failure.report().with_instance(uri.clone())))?
    }

    /// Count the payload's values against the request limit, check its
    /// numbers, and read its `@context`, all from its text.
    fn admit(self) -> Result<AdmittedBody, ExtractorRejection> {
        let ReadBody {
            bytes,
            content_kind,
            carrier,
            link_context,
            uri,
            value_limit,
            blocking_decode: _,
        } = self;
        // Counted before anything is built, so a payload of too many values
        // costs its bytes and nothing more.
        // clone: each refusal names the request, which the admitted body keeps naming
        refuse_values_past_budget(&bytes, value_limit).map_err(|refusal| ExtractorRejection::Classified(refusal.report().with_instance(uri.clone())))?;
        // clone: as above
        refuse_numbers_out_of_range(&bytes).map_err(|refusal| number_rejection(refusal, uri.clone()))?;
        // clone: as above
        let envelope = BodyEnvelope::read(&bytes).map_err(|error| number_rejection(NumberRangeError::Syntax(error), uri.clone()))?;

        // Reject empty arrays — not a valid NGSI-LD entity or batch payload
        if envelope.is_empty_array() {
            return Err(ExtractorRejection::bad_request_data("Invalid JSON: expected object, got empty array", uri));
        }

        // Clause 6.3.21: a profile `Link` announcing the payload's NGSI-LD
        // version is read by the `PayloadProfile` extractor. This broker
        // interprets every version the clause 4.3.6.8 tables describe, so no
        // amendment of the payload is needed for it to be accepted here.
        // Clause 6.3.5: an application/json body must not carry @context.
        if matches!(content_kind, ContentKind::Json) && envelope.has_context() {
            return Err(ExtractorRejection::bad_request_data("application/json body must not contain @context", uri));
        }

        if !envelope.contexts_are_well_formed() {
            return Err(ExtractorRejection::bad_request_data(
                "@context must be a string, an object, or an array of those",
                uri,
            ));
        }

        // The context is carried by the spec; the member is skipped when the
        // body is decoded into its domain type.
        let context = match carrier {
            PayloadCarrier::LdJsonBody => ContextSpec::from_body(
                envelope
                    .into_body_context()
                    .map_err(|error| ExtractorRejection::bad_request_data(error.to_string(), uri.clone()))?, // clone: as above
            ),
            PayloadCarrier::LinkHeader => ContextSpec::from_request(link_context.map_or(RequestContext::Default, RequestContext::Link)),
        };

        Ok(AdmittedBody { text: bytes, context, uri })
    }
}

/// Apply the clause 6.3.4 and 6.3.5 request preconditions and read the body.
async fn read_body<S: Send + Sync>(req: Request, state: &S) -> Result<ReadBody, ExtractorRejection> {
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

    // Clause 6.3.5: an `application/ld+json` body names its context itself and
    // refuses a context `Link` beside it; any other body is read against the
    // one context `Link` the request carries, or none. The headers are read
    // before the body is consumed.
    let carrier = match content_kind {
        ContentKind::LdJson => PayloadCarrier::LdJsonBody,
        ContentKind::Json | ContentKind::MergePatchJson => PayloadCarrier::LinkHeader,
    };
    let (mut parts, body) = req.into_parts();
    let link_context = RequestLinks::of(&mut parts)
        .and_then(|links| links.context_for(carrier))
        .map_err(|error| ExtractorRejection::Classified(classify_context_link_error(&error).with_instance(uri.clone())))?;
    let value_limit = parts.extensions.get::<RequestJsonValueLimit>().copied();
    let blocking_decode = parts.extensions.get::<BlockingDecodePolicy>().cloned(); // clone: the body owns its share of the permits
    let req = Request::from_parts(parts, body);

    let bytes = buffer_payload(req, state, declared, uri.clone()).await?;

    // A declared length the payload did not carry leaves clause 5.5.4 with
    // no JSON document to read, which is the refusal the `Content-Length`
    // precondition already owns.
    if bytes.is_empty() {
        return Err(ContentLengthPreconditionError::Empty.into_rejection(uri));
    }

    Ok(ReadBody {
        bytes,
        content_kind,
        carrier,
        link_context,
        uri,
        value_limit,
        blocking_decode,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        blocking_decode::BlockingDecodePolicy,
        jsonld::{JsonLd, JsonLdDocument},
        request_json_value_limit::RequestJsonValueLimit,
    };
    use andromeda_http_response::extraction_rejection::ExtractorRejection;
    use andromeda_jsonld::request_context::RequestContext;
    use andromeda_ngsi_ld_model::{entity::Entity, request_body::RequestBody};
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::{
        body::{Body, HttpBody, to_bytes},
        extract::FromRequest,
        http::{
            HeaderValue,
            Request,
            StatusCode,
            header::{CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, LINK},
        },
        response::IntoResponse,
    };
    use iri_rs::IriBuf;
    use serde_json::{Value, json};
    use std::num::NonZeroUsize;

    /// Declare the fixed length of a test request's body.
    fn with_content_length(mut request: Request<Body>) -> Request<Body> {
        let length = request.body().size_hint().exact().expect("a fixed test body length");
        let value = HeaderValue::from_str(&length.to_string()).expect("a valid content length");
        request.headers_mut().insert(CONTENT_LENGTH, value);
        request
    }

    async fn extract<T>(request: Request<Body>) -> Result<JsonLd<T>, ExtractorRejection>
    where
        T: RequestBody + Send + 'static,
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

    /// A JSON request carrying `body` and one `Link` line per entry of `links`.
    fn json_request_with_links(body: &'static str, links: &[&str]) -> Request<Body> {
        request_with_links("application/json", body, links)
    }

    /// A request of `content_type` carrying `body` and one `Link` line per
    /// entry of `links`, addressed to `broker.example`.
    fn request_with_links(content_type: &str, body: &'static str, links: &[&str]) -> Request<Body> {
        let mut request = Request::builder()
            .uri("/ngsi-ld/v1/entityOperations/query")
            .header("host", "broker.example")
            .header(CONTENT_TYPE, content_type);
        for link in links {
            request = request.header(LINK, *link);
        }
        request.body(Body::from(body)).expect("request builds")
    }

    /// The request context a document extraction reads, or its rejection.
    async fn extracted_context(request: Request<Body>) -> Result<RequestContext, ExtractorRejection> {
        extract_document(request).await.map(|admitted| admitted.context.request_context().clone()) // clone: the spec is dropped here
    }

    /// Whether `rejection` is a clause 5.5.4 `BadRequestData`.
    fn is_bad_request_data(rejection: &ExtractorRejection) -> bool {
        match rejection {
            ExtractorRejection::Classified(response) => response.problem().error_type() == NgsiLdErrorType::BadRequestData,
            ExtractorRejection::BadRequestData { .. } => true,
            ExtractorRejection::Empty(_) | ExtractorRejection::NotAcceptable(_) | ExtractorRejection::InvalidRequest { .. } => false,
        }
    }

    const CONTEXT_A: &str = r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;
    const CONTEXT_B: &str = r#"<https://example.com/b.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#;
    const PROFILE: &str = r#"<https://uri.etsi.org/ngsi-ld/v1.9>; rel="profile""#;

    /// Clause 6.3.5 and JSON-LD 1.1 API section 9.4.1: a JSON body read against
    /// two context links is refused as `BadRequestData`.
    #[tokio::test]
    async fn a_json_body_with_two_context_links_is_bad_request_data() {
        let request = json_request_with_links(
            r#"{"type": "Query"}"#,
            &[
                r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#,
                r#"<https://example.com/b.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#,
            ],
        );

        let rejection = extract_document(request).await.err().expect("two context links are refused");

        assert!(matches!(rejection, ExtractorRejection::Classified(_)), "{rejection:?}");
    }

    #[tokio::test]
    async fn a_json_body_with_one_context_link_is_read_against_it() {
        let request = json_request_with_links(
            r#"{"type": "Query"}"#,
            &[r#"<https://example.com/a.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#],
        );

        pretty_assertions::assert_eq!(
            extracted_context(request).await.expect("one context link is admitted"),
            RequestContext::Link(IriBuf::new("https://example.com/a.jsonld".to_owned()).expect("a valid IRI"))
        );
    }

    /// Two context values in one `Link` line are two context links.
    #[tokio::test]
    async fn a_json_body_with_two_context_values_in_one_link_line_is_bad_request_data() {
        let request = json_request_with_links(r#"{"type": "Query"}"#, &[&format!("{CONTEXT_A}, {CONTEXT_B}")]);
        let rejection = extracted_context(request).await.expect_err("two context links are refused");
        assert!(is_bad_request_data(&rejection), "{rejection:?}");
    }

    /// JSON-LD counts links, not distinct targets.
    #[tokio::test]
    async fn a_json_body_with_the_same_context_link_twice_is_bad_request_data() {
        let request = json_request_with_links(r#"{"type": "Query"}"#, &[CONTEXT_A, CONTEXT_A]);
        let rejection = extracted_context(request).await.expect_err("the same context link twice is refused");
        assert!(is_bad_request_data(&rejection), "{rejection:?}");
    }

    /// Clause 6.3.5: an `application/ld+json` body beside a context `Link` is
    /// `BadRequestData`, decided on the parsed link-values, whatever case the
    /// relation is written in.
    #[tokio::test]
    async fn an_ld_json_body_beside_a_context_link_is_bad_request_data() {
        let body = r#"{"type": "Query", "@context": "https://example.com/a.jsonld"}"#;
        for link in [CONTEXT_A, r#"<https://example.com/a.jsonld>; rel="HTTP://WWW.W3.ORG/ns/json-ld#context""#] {
            let rejection = extracted_context(request_with_links("application/ld+json", body, &[link]))
                .await
                .expect_err("a context link beside an ld+json body is refused");
            assert!(is_bad_request_data(&rejection), "{link}: {rejection:?}");
        }
    }

    /// Clause 6.3.21: a `profile` link is not a context and may accompany an
    /// `application/ld+json` body, which is then read against its own context.
    #[tokio::test]
    async fn an_ld_json_body_beside_a_profile_link_is_read_against_its_own_context() {
        let body = r#"{"type": "Query", "@context": "https://example.com/a.jsonld"}"#;
        pretty_assertions::assert_eq!(
            extracted_context(request_with_links("application/ld+json", body, &[PROFILE]))
                .await
                .expect("a profile link is admitted"),
            RequestContext::Body(json!("https://example.com/a.jsonld"))
        );
    }

    /// Clause 5.5.4: a malformed `Link` refuses a body-carrying request as it
    /// does a body-less one, whatever the body's media type.
    #[tokio::test]
    async fn a_malformed_link_refuses_a_body_carrying_request() {
        for content_type in ["application/json", "application/ld+json"] {
            let body = r#"{"type": "Query", "@context": "https://example.com/a.jsonld"}"#;
            let request = if content_type == "application/json" {
                request_with_links(content_type, r#"{"type": "Query"}"#, &["not a link"])
            } else {
                request_with_links(content_type, body, &["not a link"])
            };
            let rejection = extracted_context(request).await.expect_err("a malformed Link is refused");
            assert!(is_bad_request_data(&rejection), "{content_type}: {rejection:?}");
        }
    }

    /// A `Link` value that is not visible ASCII refuses a body-carrying request
    /// rather than being skipped.
    #[tokio::test]
    async fn a_non_ascii_link_refuses_a_body_carrying_request() {
        let mut request = json_request(r#"{"type": "Query"}"#);
        request.headers_mut().insert(
            LINK,
            HeaderValue::from_bytes("<https://example.com/\u{e9}.jsonld>; rel=\"http://www.w3.org/ns/json-ld#context\"".as_bytes())
                .expect("an opaque header value"),
        );
        let rejection = extracted_context(request).await.expect_err("a non-ASCII Link is refused");
        assert!(is_bad_request_data(&rejection), "{rejection:?}");
    }

    /// IETF RFC 8288 section 3.1: a relative context link resolves against
    /// the request's own URI.
    #[tokio::test]
    async fn a_relative_context_link_on_a_json_body_resolves_against_the_request_uri() {
        let request = json_request_with_links(r#"{"type": "Query"}"#, &[r#"<ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context""#]);
        pretty_assertions::assert_eq!(
            extracted_context(request).await.expect("a relative context link"),
            RequestContext::Link(IriBuf::new("http://broker.example/ngsi-ld/v1/entityOperations/ctx.jsonld".to_owned()).expect("a valid IRI"))
        );
    }

    /// A JSON body without a context `Link` is read against the default
    /// context.
    #[tokio::test]
    async fn a_json_body_without_a_link_is_read_against_the_default_context() {
        pretty_assertions::assert_eq!(
            extracted_context(json_request(r#"{"type": "Query"}"#)).await.expect("admitted"),
            RequestContext::Default
        );
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

    /// The members of an `application/ld+json` array body read as one request
    /// name one context; members naming different ones are refused rather
    /// than merged into a union neither of them named.
    #[tokio::test]
    async fn an_ld_json_array_body_whose_members_name_different_contexts_is_bad_request_data() {
        let body = json!([
            {"@context": "https://example.org/a", "id": "urn:ngsi-ld:T:1", "type": "T"},
            {"@context": ["https://example.org/b", "https://example.org/a"], "id": "urn:ngsi-ld:T:2", "type": "T"}
        ]);
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/ld+json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        let rejection = extract_document(req).await.err().expect("divergent contexts are refused");
        assert!(is_bad_request_data(&rejection), "{rejection:?}");
    }

    /// Members carrying one and the same context are read against it, as
    /// written.
    #[tokio::test]
    async fn an_ld_json_array_body_whose_members_share_a_context_is_read_against_it() {
        let body = json!([
            {"@context": ["https://example.org/a", "https://example.org/b", "https://example.org/a"], "id": "urn:ngsi-ld:T:1", "type": "T"},
            {"@context": ["https://example.org/a", "https://example.org/b", "https://example.org/a"], "id": "urn:ngsi-ld:T:2", "type": "T"}
        ]);
        let req = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/ld+json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();

        pretty_assertions::assert_eq!(
            extracted_context(req).await.expect("a shared context"),
            RequestContext::Body(json!(["https://example.org/a", "https://example.org/b", "https://example.org/a"]))
        );
    }

    /// Clause 6.3.5: an `application/ld+json` body without `@context` is
    /// `BadRequestData`, and so is an `application/json` body carrying one.
    #[tokio::test]
    async fn a_context_in_the_wrong_body_or_none_in_the_right_one_is_bad_request_data() {
        let without = Request::builder()
            .uri("/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/ld+json")
            .body(Body::from(r#"{"id": "urn:ngsi-ld:T:1", "type": "T"}"#))
            .unwrap();
        let rejection = extract_document(without).await.err().expect("an ld+json body needs @context");
        assert!(is_bad_request_data(&rejection), "{rejection:?}");

        let with = json_request(r#"{"id": "urn:ngsi-ld:T:1", "type": "T", "@context": "https://example.org/a"}"#);
        let rejection = extract_document(with).await.err().expect("a json body must not carry @context");
        assert!(is_bad_request_data(&rejection), "{rejection:?}");
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

    /// An Entity decoded straight from the body text renders as the
    /// document the untyped extraction parses, numbers in canonical spelling
    /// included, and both refuse the same bodies.
    #[tokio::test]
    async fn an_untyped_document_is_the_document_an_entity_typed_extraction_renders() {
        let bodies = [
            r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":{"type":"Property","value":[21.5,-7,0,18446744073709551616,0.100000000000000005]}}"#,
            r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":{"type":"Property","value":[1e-7,-2.5e-400,1E+20,1e20,0.00001,1.50]}}"#,
            r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":{"type":"Property","value":{"b":1,"$serde_json::private::Number":"5"}}}"#,
            r#"{"id":"urn:ngsi-ld:T:1","type":"T","s":"text","j":{"type":"JsonProperty","json":{"a":[1.0,{"b":2.50}]}},"raw":{"value":1E+2}}"#,
        ];
        for body in bodies {
            let untyped = extract_document(json_request(body)).await.map(|extracted| extracted.document.to_string());
            let typed = extract::<Entity>(json_request(body))
                .await
                .map(|extracted| serde_json::to_value(&extracted.value).expect("an entity renders").to_string());
            match (untyped, typed) {
                (Ok(untyped), Ok(typed)) => pretty_assertions::assert_eq!(untyped, typed, "{body}"),
                // Both refuse the document; the typed extraction can also name
                // the member it failed at, which an untyped one has none of.
                (Err(ExtractorRejection::BadRequestData { .. }), Err(ExtractorRejection::BadRequestData { .. })) => {}
                _other => panic!("{body}: the untyped and Entity-typed extractions disagree"),
            }
        }
    }

    #[tokio::test]
    async fn a_typed_body_that_does_not_decode_is_bad_request_data_naming_the_member_and_the_positioned_reader_message() {
        #[derive(Debug, serde::Deserialize)]
        struct Named {
            name: String,
        }

        impl RequestBody for Named {}

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

    /// `request` carrying the JSON value limit of `values` the stack publishes.
    fn with_value_limit(mut request: Request<Body>, values: NonZeroUsize) -> Request<Body> {
        request.extensions_mut().insert(RequestJsonValueLimit::new(JsonValueLimit::new(values)));
        request
    }

    /// `request` carrying the blocking decode threshold of `bytes` the stack
    /// publishes.
    fn with_decode_threshold(mut request: Request<Body>, bytes: usize) -> Request<Body> {
        request.extensions_mut().insert(BlockingDecodePolicy::new(bytes, NonZeroUsize::MIN));
        request
    }

    /// The status, `Connection` header and parsed problem a rejection answers
    /// with.
    async fn answered(rejection: ExtractorRejection) -> (StatusCode, Option<HeaderValue>, Value) {
        let response = rejection.into_response();
        let status = response.status();
        let connection = response.headers().get(CONNECTION).cloned(); // clone: the response is consumed for its body below
        let body = to_bytes(response.into_body(), usize::MAX).await.expect("a readable body");
        (status, connection, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    /// Clause 6.3.2: a payload holding more JSON values than the published
    /// limit is the 413 "Request Entity too large", naming the limit. The
    /// payload was read in full before it was counted, so the connection is
    /// left open.
    #[tokio::test]
    async fn a_body_past_the_json_value_limit_is_a_413_problem_naming_the_limit() {
        let three = NonZeroUsize::new(3).expect("three is not zero");
        let request = with_value_limit(json_request(r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":[1,2]}"#), three);

        let rejection = extract_document(request).await.err().expect("six values exceed three");
        let (status, connection, problem) = answered(rejection).await;

        pretty_assertions::assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge");
        pretty_assertions::assert_eq!(problem["status"], 413);
        pretty_assertions::assert_eq!(problem["instance"], "/ngsi-ld/v1/entities");
        let detail = problem["detail"].as_str().expect("a detail");
        assert!(detail.contains("3 JSON values"), "{detail}");
        pretty_assertions::assert_eq!(connection, None);
    }

    /// The typed extraction counts the payload as the untyped one does.
    #[tokio::test]
    async fn a_typed_body_past_the_json_value_limit_is_the_same_413() {
        let three = NonZeroUsize::new(3).expect("three is not zero");
        let request = with_value_limit(json_request(r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":[1,2]}"#), three);

        let rejection = extract::<Entity>(request).await.err().expect("six values exceed three");
        let (status, _connection, problem) = answered(rejection).await;

        pretty_assertions::assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge");
    }

    /// A payload holding exactly the published number of values is admitted.
    #[tokio::test]
    async fn a_body_at_the_json_value_limit_is_admitted() {
        let six = NonZeroUsize::new(6).expect("six is not zero");
        let request = with_value_limit(json_request(r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":[1,2]}"#), six);

        assert!(extract_document(request).await.is_ok());
    }

    /// A router assembled without the stack publishes no limit, and a payload
    /// read through it is not counted.
    #[tokio::test]
    async fn a_body_without_a_published_limit_is_not_budgeted() {
        let request = json_request(r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]}"#);

        assert!(extract_document(request).await.is_ok());
    }

    /// What one decode path makes of `body`: the admitted document and its
    /// context, or the status and problem of its refusal.
    async fn decoded_at(body: &'static str, threshold: usize) -> Result<(String, RequestContext), (StatusCode, Value)> {
        match extract_document(with_decode_threshold(json_request(body), threshold)).await {
            Ok(admitted) => Ok((admitted.document.to_string(), admitted.context.request_context().clone())), // clone: the spec is dropped here
            Err(rejection) => {
                let (status, _connection, problem) = answered(rejection).await;
                Err((status, problem))
            }
        }
    }

    /// Bodies every decode path admits.
    const ADMITTED_BODIES: [&str; 3] = [
        r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":{"type":"Property","value":[21.5,-7,0,18446744073709551616,0.100000000000000005]}}"#,
        r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":{"type":"Property","value":[1e-7,-2.5e-400,1E+20,1e20,0.00001,1.50]}}"#,
        r#"[{"id":"urn:ngsi-ld:T:1","type":"T"},{"id":"urn:ngsi-ld:T:2","type":"T","s":"text"}]"#,
    ];

    /// Bodies every decode path refuses.
    const REFUSED_BODIES: [&str; 5] = [
        "[1,",
        "[]",
        r#"{"id":"urn:ngsi-ld:T:1","type":"T","p":{"type":"Property","value":[1e-16384]}}"#,
        r#"{"id":"urn:ngsi-ld:T:1","type":"T","@context":"https://example.org/a"}"#,
        // The reader's private number member cannot be told from a number
        // it read, so a body spelling it out is refused.
        r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":{"type":"Property","value":{"b":1,"$serde_json::private::Number":"5"}}}"#,
    ];

    /// A threshold of zero decodes every payload on the blocking pool, and one
    /// of `usize::MAX` none: both give the same document.
    #[tokio::test]
    async fn a_body_decoded_off_the_executor_is_the_body_decoded_inline() {
        for body in ADMITTED_BODIES {
            let off_executor = decoded_at(body, 0).await;
            let inline = decoded_at(body, usize::MAX).await;
            assert!(off_executor.is_ok(), "{body}: {off_executor:?}");
            pretty_assertions::assert_eq!(off_executor, inline, "{body}");
        }
    }

    /// A payload refused while it is decoded is refused with the same status
    /// and problem on the blocking pool as inline.
    #[tokio::test]
    async fn a_malformed_body_is_refused_alike_on_both_decode_paths() {
        for body in REFUSED_BODIES {
            let off_executor = decoded_at(body, 0).await;
            let inline = decoded_at(body, usize::MAX).await;
            assert!(off_executor.is_err(), "{body}: {off_executor:?}");
            pretty_assertions::assert_eq!(off_executor, inline, "{body}");
        }
    }

    /// What the typed decode path makes of `body` as a `T`: the decoded value
    /// rendered back and its context, or the status and problem of its
    /// refusal.
    async fn typed_at(body: &'static str, threshold: usize) -> Result<(String, RequestContext), (StatusCode, Value)> {
        match extract::<Entity>(with_decode_threshold(json_request(body), threshold)).await {
            Ok(admitted) => Ok((
                serde_json::to_value(&admitted.value).expect("an entity renders").to_string(),
                admitted.context.request_context().clone(), // clone: the spec is dropped here
            )),
            Err(rejection) => {
                let (status, _connection, problem) = answered(rejection).await;
                Err((status, problem))
            }
        }
    }

    /// The typed extraction decodes every body alike on the blocking pool and
    /// inline, admitting and refusing the same objects the untyped one does;
    /// an Entity is an object, so an array body is no Entity.
    #[tokio::test]
    async fn a_typed_body_is_decoded_alike_on_both_decode_paths() {
        for body in ADMITTED_BODIES.into_iter().chain(REFUSED_BODIES) {
            let off_executor = typed_at(body, 0).await;
            pretty_assertions::assert_eq!(off_executor, typed_at(body, usize::MAX).await, "{body}");
            let untyped = decoded_at(body, 0).await;
            if body.starts_with('{') {
                pretty_assertions::assert_eq!(off_executor.is_ok(), untyped.is_ok(), "{body}");
            } else {
                assert!(off_executor.is_err(), "{body}: an array is no Entity");
            }
        }
    }
}
