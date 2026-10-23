use crate::{blocking_decode::BlockingDecodePolicy, jsonld::JsonLd, request_json_value_limit::RequestJsonValueLimit};
use andromeda_http_response::extraction_rejection::ExtractorRejection;
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    request_context::RequestContext,
    user_context_admission::{AdmittedContext, SubmittedContext},
};
use andromeda_ngsi_ld_model::{entity::Entity, request_body::RequestBody};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemDetail};
use andromeda_ngsi_ld_representation::embedded_context_admission::find_embedded_context;
use axum::{
    body::{Body, HttpBody, to_bytes},
    extract::FromRequest,
    http::{
        HeaderValue,
        Method,
        Request,
        StatusCode,
        header::{CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, LINK},
    },
    response::IntoResponse,
};
use iri_rs::IriBuf;
use serde::Deserialize;
use serde_json::{Value, json};
use std::num::NonZeroUsize;

/// A body that holds no member of what it is given: whatever object a request
/// carries decodes into it, so a test of the admission alone is not a test of
/// a model.
#[derive(Debug, Deserialize)]
struct AnyObject {}

impl RequestBody for AnyObject {}

/// A body that refuses every member but the two an Entity must carry, so a
/// top-level `@context` left in the text it is decoded from is a failure.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identified {
    /// The entity identifier.
    id: String,
    /// The entity type.
    #[serde(rename = "type")]
    entity_type: String,
}

impl RequestBody for Identified {}

/// Declare the fixed length of a test request's body.
fn with_content_length(mut request: Request<Body>) -> Request<Body> {
    let length = request.body().size_hint().exact().expect("a fixed test body length");
    let value = HeaderValue::from_str(&length.to_string()).expect("a valid content length");
    request.headers_mut().insert(CONTENT_LENGTH, value);
    request
}

/// What a typed extraction yields once its context was admitted.
struct Typed<T> {
    value: T,
    context: SubmittedContext,
}

async fn extract<T>(request: Request<Body>) -> Result<Typed<T>, ExtractorRejection>
where
    T: RequestBody + Send + 'static,
{
    let JsonLd { body, context } = JsonLd::<T>::from_request(with_content_length(request), &()).await?;
    let value = body.admitted(&AdmittedContext::core_only()).await.map_err(ExtractorRejection::Classified)?;
    Ok(Typed { value, context })
}

/// The admission of `request` alone, whatever it decodes as: its held body and
/// context, or the refusal of a precondition.
async fn admit(request: Request<Body>) -> Result<JsonLd<AnyObject>, ExtractorRejection> {
    JsonLd::<AnyObject>::from_request(with_content_length(request), &()).await
}

/// The rejection `request` is refused with.
async fn refused(request: Request<Body>) -> ExtractorRejection {
    match admit(request).await {
        Err(rejection) => rejection,
        Ok(_) => panic!("the request was admitted"),
    }
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

/// A request of `content_type` carrying the serialization of `body`.
fn request_of(content_type: &str, body: &Value) -> Request<Body> {
    Request::builder()
        .uri("/ngsi-ld/v1/entities")
        .header(CONTENT_TYPE, content_type)
        .body(Body::from(serde_json::to_vec(body).expect("a serializable body")))
        .expect("request builds")
}

/// The request context an extraction reads, or its rejection.
async fn extracted_context(request: Request<Body>) -> Result<SubmittedContext, ExtractorRejection> {
    admit(request).await.map(|admitted| admitted.context)
}

/// The context a request naming `context` submits.
fn submitted(context: RequestContext) -> SubmittedContext {
    SubmittedContext::from(ContextSpec::from_request(context))
}

/// A context body literal.
fn literal(value: &Value) -> JsonLiteral {
    JsonLiteral::admit(&value.to_string()).expect("an admissible literal")
}

/// Whether `rejection` is a clause 5.5.4 `BadRequestData`.
fn is_bad_request_data(rejection: &ExtractorRejection) -> bool {
    error_type_of(rejection) == Some(NgsiLdErrorType::BadRequestData)
}

/// The error type `rejection` answers with, or `None` for a bare status or the
/// `406` list.
fn error_type_of(rejection: &ExtractorRejection) -> Option<NgsiLdErrorType> {
    rejection.problem().map(ProblemDetail::error_type)
}

/// The detail `rejection` answers with.
fn detail_of(rejection: &ExtractorRejection) -> &str {
    rejection.problem().map_or("", ProblemDetail::detail)
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

    let rejection = admit(request).await.err().expect("two context links are refused");

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
        submitted(RequestContext::Link(
            IriBuf::new("https://example.com/a.jsonld".to_owned()).expect("a valid IRI")
        ))
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
        submitted(RequestContext::Body(literal(&json!("https://example.com/a.jsonld"))))
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
        submitted(RequestContext::Link(
            IriBuf::new("http://broker.example/ngsi-ld/v1/entityOperations/ctx.jsonld".to_owned()).expect("a valid IRI")
        ))
    );
}

/// A JSON body without a context `Link` is read against the default
/// context.
#[tokio::test]
async fn a_json_body_without_a_link_is_read_against_the_default_context() {
    pretty_assertions::assert_eq!(
        extracted_context(json_request(r#"{"type": "Query"}"#)).await.expect("admitted"),
        submitted(RequestContext::Default)
    );
}

#[tokio::test]
async fn rejects_unsupported_media_type() {
    let req = Request::builder()
        .uri("/ngsi-ld/v1/entities")
        .header(CONTENT_TYPE, "application/xml")
        .body(Body::from("{}"))
        .expect("request builds");

    match admit(req).await {
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
        .expect("request builds");

    match admit(req).await {
        Err(ExtractorRejection::Empty(status)) => {
            pretty_assertions::assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
        }
        _ => panic!("expected 415 for json-patch+json"),
    }
}

/// Clause 6.3.4: `application/merge-patch+json` is admitted for PATCH alone.
#[tokio::test]
async fn accepts_merge_patch_json_on_patch() {
    let mut req = request_of("application/merge-patch+json", &json!({"id": "urn:ngsi-ld:T:1", "type": "T"}));
    *req.method_mut() = Method::PATCH;

    assert!(admit(req).await.is_ok(), "merge-patch+json should be accepted on PATCH");
}

/// Clause 6.3.4: on POST and PUT the same media type is a bare 415.
#[tokio::test]
async fn rejects_merge_patch_json_on_post_and_put() {
    for method in [Method::POST, Method::PUT] {
        let mut req = request_of("application/merge-patch+json", &json!({"id": "urn:ngsi-ld:T:1", "type": "T"}));
        *req.method_mut() = method.clone();

        assert!(
            matches!(admit(req).await, Err(ExtractorRejection::Empty(StatusCode::UNSUPPORTED_MEDIA_TYPE))),
            "{method}"
        );
    }
}

#[tokio::test]
async fn rejects_empty_body_with_bad_request() {
    let req = Request::builder()
        .uri("/ngsi-ld/v1/entities")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::empty())
        .expect("request builds");

    let rejection = refused(req).await;
    pretty_assertions::assert_eq!(error_type_of(&rejection), Some(NgsiLdErrorType::InvalidRequest));
}

#[tokio::test]
async fn rejects_valid_empty_array_as_bad_request_data() {
    let req = Request::builder()
        .uri("/ngsi-ld/v1/entities")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from("[]"))
        .expect("request builds");

    let rejection = refused(req).await;
    pretty_assertions::assert_eq!(error_type_of(&rejection), Some(NgsiLdErrorType::BadRequestData));
    pretty_assertions::assert_eq!(detail_of(&rejection), "request body is an empty array, expected a JSON object");
}

/// Clause 5.5.4: a body that is no JSON document is named in the words every
/// other body reader uses.
#[tokio::test]
async fn a_body_that_is_not_json_names_itself_as_every_reader_does() {
    let rejection = refused(json_request("{")).await;
    pretty_assertions::assert_eq!(error_type_of(&rejection), Some(NgsiLdErrorType::InvalidRequest));
    assert!(
        detail_of(&rejection).starts_with("request body is not a JSON document:"),
        "{}",
        detail_of(&rejection)
    );
}

/// Clause 6.3.5: an `application/json` body carries its context in a `Link`
/// header, so an `@context` member in it is refused.
#[tokio::test]
async fn a_json_body_carrying_a_context_member_is_bad_request_data() {
    let rejection = refused(json_request(r#"{"@context":"https://example.org/ctx","id":"urn:ngsi-ld:T:1","type":"T"}"#)).await;
    pretty_assertions::assert_eq!(error_type_of(&rejection), Some(NgsiLdErrorType::BadRequestData));
    pretty_assertions::assert_eq!(detail_of(&rejection), "an application/json request body must not contain @context");
}

/// Clause 6.3.5: the context an `application/ld+json` body names is read from
/// its `@context` member, which the body is decoded without.
#[tokio::test]
async fn extracts_context_from_body() {
    let req = request_of(
        "application/ld+json",
        &json!({
            "@context": "http://example.org/context",
            "id": "urn:ngsi-ld:T:1",
            "type": "T"
        }),
    );

    let extracted = extract::<Identified>(with_content_length(req))
        .await
        .expect("the body decodes without its context");

    pretty_assertions::assert_eq!(
        extracted.context,
        submitted(RequestContext::Body(literal(&json!("http://example.org/context"))))
    );
    pretty_assertions::assert_eq!((extracted.value.id.as_str(), extracted.value.entity_type.as_str()), ("urn:ngsi-ld:T:1", "T"));
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

    let rejection = admit(request_of("application/ld+json", &body))
        .await
        .err()
        .expect("divergent contexts are refused");
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

    pretty_assertions::assert_eq!(
        extracted_context(request_of("application/ld+json", &body)).await.expect("a shared context"),
        submitted(RequestContext::Body(literal(&json!([
            "https://example.org/a",
            "https://example.org/b",
            "https://example.org/a"
        ]))))
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
        .expect("request builds");
    let rejection = admit(without).await.err().expect("an ld+json body needs @context");
    assert!(is_bad_request_data(&rejection), "{rejection:?}");

    let with = json_request(r#"{"id": "urn:ngsi-ld:T:1", "type": "T", "@context": "https://example.org/a"}"#);
    let rejection = admit(with).await.err().expect("a json body must not carry @context");
    assert!(is_bad_request_data(&rejection), "{rejection:?}");
}

#[tokio::test]
async fn a_context_that_is_not_a_string_object_or_array_is_bad_request_data() {
    // Clause 5.5.4: a `@context` shaped outside JSON-LD section 3.1 is
    // syntactically incorrect data.
    for context in [json!(42), json!(true), json!([1, "https://example.org/ctx"])] {
        let body = json!({"@context": context, "id": "urn:ngsi-ld:T:1", "type": "T"});
        let result = admit(request_of("application/ld+json", &body)).await;
        pretty_assertions::assert_eq!(
            result.err().as_ref().and_then(error_type_of),
            Some(NgsiLdErrorType::BadRequestData),
            "{context}"
        );
    }
}

#[tokio::test]
async fn content_type_with_charset_param_works() {
    let req = request_of("application/json; charset=utf-8", &json!({"id": "urn:ngsi-ld:T:1", "type": "T"}));

    assert!(admit(req).await.is_ok());
}

/// A number the body spells beyond binary64 reaches the model in its canonical
/// spelling, not through a `f64`.
#[tokio::test]
async fn nested_numbers_are_preserved_without_a_binary64_hop() {
    let req = Request::builder()
        .uri("/ngsi-ld/v1/entities")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"id":"urn:ngsi-ld:T:1","type":"T","property":{"type":"Property","value":0.100000000000000005,"nested":[18446744073709551616]},"json":{"type":"JsonProperty","json":{"n":1.234567890123456789e20}}}"#,
        ))
        .expect("request builds");

    let extracted = extract::<Entity>(req).await.expect("supported exact numbers");
    let encoded = serde_json::to_string(&extracted.value).expect("exact JSON");
    assert!(encoded.contains("0.100000000000000005"), "{encoded}");
    assert!(encoded.contains("18446744073709551616"), "{encoded}");
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

    let rejection = refused(req).await;
    pretty_assertions::assert_eq!(error_type_of(&rejection), Some(NgsiLdErrorType::BadRequestData));
    assert!(detail_of(&rejection).contains("/property/value/nested/0"), "{}", detail_of(&rejection));
}

/// Clause 5.5.4: data that is not syntactically correct is `BadRequestData`,
/// and IETF RFC 8259 section 4 asks the member names of an object to be
/// unique. Every body passes this admission, whatever it decodes as, so an
/// entity, a fragment, a subscription and a query are refused alike, the
/// refusal naming the repeated member and where it stands.
#[tokio::test]
async fn a_member_named_twice_in_any_object_of_a_body_is_bad_request_data_naming_it() {
    for (body, location) in [
        (r#"{"id":"urn:ngsi-ld:T:1","type":"T","id":"urn:ngsi-ld:T:2"}"#, "/id"),
        (
            r#"{"id":"urn:ngsi-ld:T:1","type":"T","speed":{"type":"Property","value":1},"speed":{"type":"Property","value":2}}"#,
            "/speed",
        ),
        (
            r#"{"id":"urn:ngsi-ld:T:1","type":"T","speed":{"type":"Property","value":1,"observedAt":"a","observedAt":"b"}}"#,
            "/speed/observedAt",
        ),
        (
            r#"{"id":"urn:ngsi-ld:T:1","type":"T","speed":{"type":"Property","value":{"a":{"b":1,"b":2}}}}"#,
            "/speed/value/a/b",
        ),
        (
            r#"{"id":"urn:ngsi-ld:T:1","type":"T","speed":{"type":"Property","value":[{"a":1},{"a":1,"a":2}]}}"#,
            "/speed/value/1/a",
        ),
        (
            r#"{"type":"Subscription","entities":[{"type":"T"}],"notification":{"endpoint":{"uri":"http://a"}},"notification":{"endpoint":{"uri":"http://b"}}}"#,
            "/notification",
        ),
        (r#"{"type":"Query","entities":[{"type":"T","type":"U"}]}"#, "/entities/0/type"),
    ] {
        let request = json_request(body);
        match admit(request).await {
            Err(rejection) => {
                pretty_assertions::assert_eq!(error_type_of(&rejection), Some(NgsiLdErrorType::BadRequestData), "{body}");
                assert!(
                    detail_of(&rejection).contains(&format!("the member at {location} is named twice")),
                    "{body}: {}",
                    detail_of(&rejection)
                );
            }
            Ok(_) => panic!("{body}: a repeated member was admitted"),
        }
    }
}

/// A body whose objects repeat no member name is admitted.
#[tokio::test]
async fn a_body_whose_objects_repeat_no_member_is_admitted() {
    let body = r#"{"id":"urn:ngsi-ld:T:1","type":"T","a":{"type":"Property","value":{"x":1}},"b":{"type":"Property","value":{"x":2}}}"#;
    assert!(admit(json_request(body)).await.is_ok());
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

    match JsonLd::<AnyObject>::from_request(req, &()).await {
        Err(ExtractorRejection::Empty(status)) => pretty_assertions::assert_eq!(status, StatusCode::LENGTH_REQUIRED),
        _other => panic!("expected a bare 411"),
    }
}

#[tokio::test]
async fn a_typed_body_that_does_not_decode_is_bad_request_data_naming_the_member_and_the_positioned_reader_message() {
    #[derive(Debug, Deserialize)]
    struct Named {
        name: String,
    }

    impl RequestBody for Named {}

    match extract::<Named>(json_request(r#"{"name":5}"#)).await {
        Err(ExtractorRejection::Classified(response)) => {
            pretty_assertions::assert_eq!(response.problem().error_type(), NgsiLdErrorType::BadRequestData);
            pretty_assertions::assert_eq!(
                response.problem().detail(),
                "the request body member 'name' does not decode: invalid type: integer `5`, expected a string at line 1 column 9"
            );
        }
        Ok(extracted) => panic!("accepted the name {}", extracted.value.name),
        Err(other) => panic!("expected BadRequestData, got {other:?}"),
    }
}

/// A failing member below the top level is named by its whole path.
#[tokio::test]
async fn a_nested_member_that_does_not_decode_is_named_by_its_path() {
    #[derive(Debug, Deserialize)]
    struct Endpoint {
        port: u16,
    }

    #[derive(Debug, Deserialize)]
    struct Hooked {
        endpoint: Endpoint,
    }

    impl RequestBody for Hooked {}

    match extract::<Hooked>(json_request(r#"{"endpoint":{"port":"x"}}"#)).await {
        Err(ExtractorRejection::Classified(response)) => {
            assert!(
                response
                    .problem()
                    .detail()
                    .starts_with("the request body member 'endpoint.port' does not decode: "),
                "{}",
                response.problem().detail()
            );
        }
        Ok(extracted) => panic!("accepted the port {}", extracted.value.endpoint.port),
        Err(other) => panic!("expected BadRequestData, got {other:?}"),
    }
}

/// A body whose failure is its own `@context` member is not blamed on that
/// member: the context is no part of the resource, and the refusal names what
/// the decoding failed at.
#[tokio::test]
async fn a_failure_is_never_blamed_on_the_context_the_body_was_read_without() {
    let req = request_of("application/ld+json", &json!({"@context": "https://example.org/a", "id": 5, "type": "T"}));

    match extract::<Identified>(with_content_length(req)).await {
        Err(ExtractorRejection::Classified(response)) => {
            let detail = response.problem().detail();
            assert!(detail.contains("'id'"), "{detail}");
            assert!(!detail.contains("@context"), "{detail}");
        }
        Ok(extracted) => panic!("accepted the id {}", extracted.value.id),
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

    let rejection = admit(request).await.err().expect("six values exceed three");
    let (status, connection, problem) = answered(rejection).await;

    pretty_assertions::assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/RequestEntityTooLarge");
    pretty_assertions::assert_eq!(problem["status"], 413);
    pretty_assertions::assert_eq!(problem["instance"], "/ngsi-ld/v1/entities");
    let detail = problem["detail"].as_str().expect("a detail");
    assert!(detail.contains("3 JSON values"), "{detail}");
    pretty_assertions::assert_eq!(connection, None);
}

/// A typed extraction counts the payload as the admission does.
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

    assert!(admit(request).await.is_ok());
}

/// A router assembled without the stack publishes no limit, and a payload
/// read through it is not counted.
#[tokio::test]
async fn a_body_without_a_published_limit_is_not_budgeted() {
    let request = json_request(r#"{"id":"urn:ngsi-ld:T:1","type":"T","n":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]}"#);

    assert!(admit(request).await.is_ok());
}

/// What one decode path makes of `body`: the admitted context, or the status
/// and problem of its refusal.
async fn admitted_at(body: &'static str, threshold: usize) -> Result<SubmittedContext, (StatusCode, Value)> {
    match admit(with_decode_threshold(json_request(body), threshold)).await {
        Ok(admitted) => Ok(admitted.context),
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

/// A threshold of zero admits every payload on the blocking pool, and one
/// of `usize::MAX` none: both give the same context.
#[tokio::test]
async fn a_body_admitted_off_the_executor_is_the_body_admitted_inline() {
    for body in ADMITTED_BODIES {
        let off_executor = admitted_at(body, 0).await;
        let inline = admitted_at(body, usize::MAX).await;
        assert!(off_executor.is_ok(), "{body}: {off_executor:?}");
        pretty_assertions::assert_eq!(off_executor, inline, "{body}");
    }
}

/// A payload refused while it is admitted is refused with the same status
/// and problem on the blocking pool as inline.
#[tokio::test]
async fn a_malformed_body_is_refused_alike_on_both_decode_paths() {
    for body in REFUSED_BODIES {
        let off_executor = admitted_at(body, 0).await;
        let inline = admitted_at(body, usize::MAX).await;
        assert!(off_executor.is_err(), "{body}: {off_executor:?}");
        pretty_assertions::assert_eq!(off_executor, inline, "{body}");
    }
}

/// What the typed decode path makes of `body` as an Entity: the decoded value
/// rendered back and its context, or the status and problem of its refusal.
async fn typed_at(body: &'static str, threshold: usize) -> Result<(String, SubmittedContext), (StatusCode, Value)> {
    match extract::<Entity>(with_decode_threshold(json_request(body), threshold)).await {
        Ok(admitted) => Ok((serde_json::to_value(&admitted.value).expect("an entity renders").to_string(), admitted.context)),
        Err(rejection) => {
            let (status, _connection, problem) = answered(rejection).await;
            Err((status, problem))
        }
    }
}

/// The typed extraction decodes every body alike on the blocking pool and
/// inline; an Entity is an object, so an array body is no Entity.
#[tokio::test]
async fn a_typed_body_is_decoded_alike_on_both_decode_paths() {
    for body in ADMITTED_BODIES.into_iter().chain(REFUSED_BODIES) {
        let off_executor = typed_at(body, 0).await;
        pretty_assertions::assert_eq!(off_executor, typed_at(body, usize::MAX).await, "{body}");
        let admitted = admitted_at(body, 0).await;
        if body.starts_with('{') {
            pretty_assertions::assert_eq!(off_executor.is_ok(), admitted.is_ok(), "{body}");
        } else {
            assert!(off_executor.is_err(), "{body}: an array is no Entity");
        }
    }
}

/// Clause 5.5.7: a Subscription carrying an `@context` inside `entities`
/// is extracted with the finding located, for the admission to refuse.
#[tokio::test]
async fn a_subscription_embedding_a_context_carries_the_finding() {
    let body = r#"{"type": "Subscription", "entities": [{"type": "Device", "@context": {"Device": "https://example.org/Device"}}], "notification": {"endpoint": {"uri": "http://example.org/notify"}}}"#;
    let extracted = admit(json_request(body)).await.expect("the body is admitted for the admission to judge");
    let expected = find_embedded_context(body.as_bytes()).expect("valid JSON");
    pretty_assertions::assert_eq!(expected.as_ref().map(|found| found.path.to_string()).as_deref(), Some("/entities/0"));
    pretty_assertions::assert_eq!(
        extracted.context,
        SubmittedContext::new(ContextSpec::from_request(RequestContext::Default), expected)
    );
}
