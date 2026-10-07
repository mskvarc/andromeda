//! Add @context (clause 5.13.2, binding 6.29.3.1).

use crate::{
    broker_endpoints::broker_url,
    context_api::{cached_urls, create_context, extract_id, get_context, list_contexts},
    context_origin::ContextOrigin,
    ngsi_requests::{create_entity, create_subscription, delete_entity, delete_subscription},
    polling::eventually,
    private_tenant::PrivateTenant,
    problem_types::BAD_REQUEST_DATA_TYPE,
    unique_ids::unique_id,
};
use reqwest::StatusCode;
use serde_json::json;

/// A key-value dictionary is added as a Hosted @context.
#[tokio::test]
async fn add_valid_context_dictionary_returns_201() {
    let tenant = PrivateTenant::provisioned().await;
    let body = json!({
        "@context": {
            "A1": "urn:ngsi-ld:attributes:A1",
            "A2": "urn:ngsi-ld:attributes:A2",
            "A3": "urn:ngsi-ld:attributes:A3"
        }
    });

    let (status, location, _) = create_context(tenant.client(), &body).await;

    assert_eq!(status, StatusCode::CREATED);
    let context_id = extract_id(location.as_deref().expect("a Location header"));
    let (status, detail) = get_context(tenant.client(), &context_id, "details=true").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["kind"], "Hosted");
    tenant.finish().await;
}

/// A list of URLs is added as a Hosted @context; the documents it
/// names are served by the test's own origin.
#[tokio::test]
async fn add_valid_context_uri_list_returns_201() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let first = origin
        .serve_document("/first.jsonld", &json!({ "@context": { "A1": "urn:ngsi-ld:attributes:A1" } }))
        .await;
    let second = origin
        .serve_document("/second.jsonld", &json!({ "@context": { "A2": "urn:ngsi-ld:attributes:A2" } }))
        .await;

    let (status, location, _) = create_context(tenant.client(), &json!({ "@context": [first, second] })).await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(location.is_some(), "a Location header");
    origin.shutdown().await;
    tenant.finish().await;
}

/// Refuse `body` on Add @context as `BadRequestData` and store nothing.
///
/// Clause 5.13.2.4 applies clause 5.5.4 "in case of invalid @context": each
/// body is valid JSON, so a local context it does not carry is data that "is
/// not syntactically correct", which is `BadRequestData`, not `InvalidRequest`.
async fn assert_refused_as_bad_request_data(body: &serde_json::Value) {
    let tenant = PrivateTenant::provisioned().await;

    let (status, location, problem) = create_context(tenant.client(), body).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {problem}");
    assert_eq!(problem["type"], BAD_REQUEST_DATA_TYPE.as_str(), "{body}: {problem}");
    assert_eq!(location, None, "{body} must not be given a Location");
    let (status, hosted) = list_contexts(tenant.client(), "kind=Hosted").await;
    assert_eq!(status, StatusCode::OK);
    assert!(hosted.is_empty(), "{body} must store nothing: {hosted:?}");
    tenant.finish().await;
}

/// A body without a top-level `@context` is `BadRequestData`.
#[tokio::test]
async fn add_context_without_a_context_member_is_bad_request_data() {
    assert_refused_as_bad_request_data(&json!({
        "A1": "urn:ngsi-ld:attributes:A1",
        "A2": "urn:ngsi-ld:attributes:A2",
        "A3": "urn:ngsi-ld:attributes:A3"
    }))
    .await;
}

/// An empty object carries no `@context` and is `BadRequestData`.
#[tokio::test]
async fn add_context_empty_object_is_bad_request_data() {
    assert_refused_as_bad_request_data(&json!({})).await;
}

/// Clause 5.13.2.3: the payload is "a JSON object that has a top-level field
/// named @context"; an array holding one is not that object.
#[tokio::test]
async fn add_context_array_body_is_bad_request_data() {
    assert_refused_as_bad_request_data(&json!([{ "@context": { "A1": "urn:ngsi-ld:attributes:A1" } }])).await;
}

/// Clause 5.13.2.3 with JSON-LD 1.1 clause 9.15: a number is no context, so a
/// top-level `@context` holding one carries no local context to store.
#[tokio::test]
async fn add_context_member_that_is_a_number_is_bad_request_data() {
    assert_refused_as_bad_request_data(&json!({ "@context": 5 })).await;
}

/// JSON-LD 1.1 clause 9.15: a context array holds `null`, strings and objects,
/// never a number or another array.
#[tokio::test]
async fn add_context_array_holding_no_context_is_bad_request_data() {
    assert_refused_as_bad_request_data(&json!({ "@context": ["https://example.org/a.jsonld", 5] })).await;
    assert_refused_as_bad_request_data(&json!({ "@context": [["https://example.org/a.jsonld"]] })).await;
}

/// Clause 5.5.4 with IETF RFC 8259 section 4: a local context naming one term
/// twice is data that is not syntactically correct, `BadRequestData`, and
/// nothing is stored.
#[tokio::test]
async fn add_context_naming_a_term_twice_is_bad_request_data() {
    let tenant = PrivateTenant::provisioned().await;
    let body = r#"{"@context":{"A1":"urn:ngsi-ld:attributes:A1","A1":"urn:ngsi-ld:attributes:A2"}}"#;

    let response = tenant
        .client()
        .post(broker_url("jsonldContexts"))
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .expect("create_context request failed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let problem: serde_json::Value = response.json().await.expect("a problem details body");
    assert_eq!(problem["type"], BAD_REQUEST_DATA_TYPE.as_str(), "{problem}");
    let (status, hosted) = list_contexts(tenant.client(), "kind=Hosted").await;
    assert_eq!(status, StatusCode::OK);
    assert!(hosted.is_empty(), "nothing is stored: {hosted:?}");
    tenant.finish().await;
}

/// A subscription whose `@context` is an array leaves an
/// `ImplicitlyCreated` @context behind (clause 5.13.1).
#[tokio::test]
async fn add_implicitly_created_context_via_subscription() {
    let tenant = PrivateTenant::provisioned().await;
    let subscription = json!({
        "type": "Subscription",
        "entities": [{ "type": "T1" }],
        "notification": { "endpoint": { "uri": "http://127.0.0.1:1/notify" } },
        "@context": [{ "T1": "https://example.org/add/T1" }, { "speed": "https://example.org/add/speed" }]
    });

    let (status, location) = create_subscription(tenant.client(), &subscription).await;

    assert_eq!(status, StatusCode::CREATED);
    let (status, contexts) = list_contexts(tenant.client(), "kind=ImplicitlyCreated").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!contexts.is_empty(), "at least one ImplicitlyCreated @context");
    delete_subscription(tenant.client(), &extract_id(location.as_deref().expect("a Location header"))).await;
    tenant.finish().await;
}

/// A Hosted @context naming a remote URL, used by a write, has the
/// broker download that URL and record it as Cached; the Hosted entry itself
/// is resolved from storage.
#[tokio::test]
async fn add_hosted_context_with_cached_uri_refs() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let context_url = origin
        .serve_document("/context.jsonld", &json!({ "@context": { "X1": "urn:ngsi-ld:attributes:X1" } }))
        .await;
    let (status, location, _) = create_context(tenant.client(), &json!({ "@context": [context_url] })).await;
    assert_eq!(status, StatusCode::CREATED);
    let hosted_id = extract_id(location.as_deref().expect("a Location header"));
    let (status, detail) = get_context(tenant.client(), &hosted_id, "details=true").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["kind"], "Hosted");
    let hosted_url = detail["URL"].as_str().expect("a serving URL").to_owned();

    let entity_id = unique_id("urn:ngsi-ld:Testing:cached-ref");
    let entity = json!({
        "id": entity_id,
        "type": "Testing",
        "X1": { "type": "Property", "value": "test" },
        "@context": [hosted_url]
    });
    let (status, _) = create_entity(tenant.client(), &entity).await;

    assert_eq!(status, StatusCode::CREATED);
    let client = tenant.client();
    let referenced = &context_url;
    eventually("the referenced URL is recorded as Cached", move || async move {
        cached_urls(client).await.contains(referenced).then_some(())
    })
    .await;
    assert!(
        !cached_urls(tenant.client()).await.contains(&hosted_url),
        "the broker's own document is never recorded as Cached"
    );
    delete_entity(tenant.client(), &entity_id).await;
    origin.shutdown().await;
    tenant.finish().await;
}

/// A single URL string is added as a Hosted @context.
#[tokio::test]
async fn add_context_single_string_uri_returns_201() {
    let tenant = PrivateTenant::provisioned().await;
    let origin = ContextOrigin::start().await;
    let context_url = origin
        .serve_document("/single.jsonld", &json!({ "@context": { "A1": "urn:ngsi-ld:attributes:A1" } }))
        .await;

    let (status, location, _) = create_context(tenant.client(), &json!({ "@context": context_url })).await;

    assert_eq!(status, StatusCode::CREATED);
    assert!(location.is_some(), "a Location header");
    origin.shutdown().await;
    tenant.finish().await;
}
