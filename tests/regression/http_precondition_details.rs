//! The HTTP request preconditions of ETSI GS CIM 009 v1.9.1 clause 6.3.4.
//!
//! Four defects, each asserted against the sentence that fixes it. The `406`
//! carried the status the clause names and nothing else, where the clause
//! requires that "the body of the message shall contain the list of the
//! available representations of the resources". The `411` was raised only by
//! the JSON-LD body extractor, leaving the batch operations and the hosted
//! `@context` creation to answer a request that declares no `Content-Length`
//! as if it had declared an unusable payload, where the clause requires that
//! "'Content-Length' HTTP header absence, shall result in just a 411 HTTP
//! status code (without any payload body)".
//!
//! Content negotiation broke an equal-weight tie by the order the client wrote
//! its `Accept` entries in, where the clause says "the order of the list above
//! is significant [...] the first one of the list shall be selected"; and it
//! refused `application/*` with a 406, where the clause admits any "media range
//! that can be expanded to" one of the three.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type clause 6.3.4 gives a request that fails a precondition
/// without the clause exempting it from a payload body.
const INVALID_REQUEST: &str = "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest";

/// Every clause 6.2 batch operation that carries a payload body: the provision
/// operations of clauses 5.6.7 to 5.6.9 and 5.6.20, whose payload is an array
/// of entity representations, clause 5.6.10 Delete Entities, whose payload is
/// an array of identifiers, and the clause 5.7.2 query, whose payload is a
/// Query. Clause 6.3.2's precondition is a property of the request, so it holds
/// for all of them regardless of which extractor reads the body.
const BODY_CARRYING_ENTITY_OPERATIONS: [&str; 6] = [
    "entityOperations/create",
    "entityOperations/upsert",
    "entityOperations/update",
    "entityOperations/merge",
    "entityOperations/delete",
    "entityOperations/query",
];

/// The `406` body of a request whose `Accept` names nothing the endpoint can
/// answer with.
async fn refusal_body(client: &TestClient, path: &str, accept: &str) -> Value {
    let response = client
        .raw(Method::GET, path)
        .header("accept", accept)
        .send()
        .await
        .expect("unacceptable Accept request");
    assert_eq!(
        response.status,
        StatusCode::NOT_ACCEPTABLE,
        "GET {path} with Accept: {accept} is a clause 6.3.4 refusal"
    );
    response.json_value().expect("clause 6.3.4 requires a body on a 406")
}

/// The media types a `406` body advertised, in the order it published them.
fn advertised(body: &Value) -> Vec<&str> {
    body["availableRepresentations"]
        .as_array()
        .expect("clause 6.3.4 requires the list of available representations")
        .iter()
        .map(|media_type| media_type.as_str().expect("a media type is text"))
        .collect()
}

/// Send a POST that declares no `Content-Length` at all: with no body attached,
/// the client neither sizes the request nor announces a length for it.
async fn post_without_declared_length(client: &TestClient, path: &str) -> TestResponse {
    client
        .raw(Method::POST, path)
        .header("content-type", "application/json")
        .send()
        .await
        .expect("request without a declared length")
}

/// The `Content-Type` of a response, which names the representation clause
/// 6.3.4 negotiated.
fn content_type(response: &TestResponse) -> Option<&str> {
    response.headers.get("content-type").and_then(|value| value.to_str().ok())
}

/// Send a GET under one `Accept`, without asking for any particular outcome.
async fn get_accepting(client: &TestClient, path: &str, accept: &str) -> TestResponse {
    client
        .raw(Method::GET, path)
        .header("accept", accept)
        .send()
        .await
        .expect("request with an Accept header")
}

/// Send a POST that declares an empty payload body.
async fn post_declaring_zero_length(client: &TestClient, path: &str) -> TestResponse {
    client
        .raw(Method::POST, path)
        .header("content-type", "application/json")
        .header("content-length", "0")
        .send()
        .await
        .expect("request declaring a zero-length body")
}

/// Clause 6.3.4: an `Accept` that "does not imply 'application/json' nor
/// 'application/ld+json' shall result in a 406 HTTP status code and the body of
/// the message shall contain the list of the available representations of the
/// resources". Retrieve Entity is one of the two operations clause 6.3.4 admits
/// `application/geo+json` for, so its list names all three.
#[tokio::test]
async fn regression_not_acceptable_an_entity_endpoint_publishes_all_three_representations() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let path = format!("entities/{}", scope.id("Building:not-acceptable"));
    let response = client
        .raw(Method::GET, &path)
        .header("accept", "application/xml")
        .send()
        .await
        .expect("unacceptable Accept request");

    assert_eq!(response.status, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(
        response.headers.get("content-type").and_then(|value| value.to_str().ok()),
        Some("application/json"),
        "clause 6.3.3 sends error bodies as application/json"
    );
    assert_eq!(
        response.json_value().expect("clause 6.3.4 requires a body on a 406"),
        json!({"availableRepresentations": ["application/json", "application/ld+json", "application/geo+json"]})
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.4: outside "Retrieve Entity" and "Query Entity", an
/// `application/geo+json` request "will result in a Not Acceptable Media Type
/// error, returning a 406 HTTP status code and the body of the message shall
/// contain the list of the available representations of the resources". The
/// list must not offer the media type just refused.
#[tokio::test]
async fn regression_not_acceptable_a_non_geojson_endpoint_refuses_geo_json_with_its_own_list() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let subscription = format!("subscriptions/{}", scope.id("Subscription:not-acceptable"));
    let temporal = "temporal/entities?type=Vehicle&timerel=after&timeAt=2020-08-01T12:00:00Z";
    for path in ["types", subscription.as_str(), "csourceRegistrations?type=Building", temporal] {
        let body = refusal_body(client, path, "application/geo+json").await;
        assert_eq!(
            body,
            json!({"availableRepresentations": ["application/json", "application/ld+json"]}),
            "GET {path}"
        );
        assert!(
            !advertised(&body).contains(&"application/geo+json"),
            "GET {path} must not advertise the representation it refused"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.15 defines the `GeoJSON` representation of entities only, so the
/// advertised list is the refusing endpoint's own: the same unacceptable
/// `Accept` yields three representations from an entity resource and two from a
/// resource that has no `GeoJSON` form.
#[tokio::test]
async fn regression_not_acceptable_the_published_list_follows_the_endpoint() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let entity_resource = refusal_body(client, "entities?type=Building", "application/xml").await;
    let type_resource = refusal_body(client, "types", "application/xml").await;

    assert_eq!(
        advertised(&entity_resource),
        vec!["application/json", "application/ld+json", "application/geo+json"]
    );
    assert_eq!(advertised(&type_resource), vec!["application/json", "application/ld+json"]);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.4: "'Content-Length' HTTP header absence, shall result in just a
/// 411 HTTP status code (without any payload body)". Create Entity (clause
/// 5.6.1) already answered this way; the assertion guards it against
/// regression while the same rule is extended to its neighbours.
#[tokio::test]
async fn regression_length_required_creating_an_entity_without_a_declared_length() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let response = post_without_declared_length(client, "entities").await;

    assert_eq!(response.status, StatusCode::LENGTH_REQUIRED);
    assert!(response.body.is_empty(), "clause 6.3.4 forbids a payload body on a 411");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.4 applies its preconditions to every "HTTP POST, PATCH and PUT"
/// request, and the batch operations are HTTP POSTs with a payload body. Each
/// of those that reads its body through the batch extractor answered a request
/// declaring no `Content-Length` as an unsupported media type or an unreadable
/// payload instead of "just a 411 HTTP status code (without any payload body)".
///
/// `entityOperations/query` is the exception and is asserted here as a guard
/// rather than as a fix: its clause 5.2.23 body reaches the JSON-LD extractor,
/// which held the precondition already.
#[tokio::test]
async fn regression_length_required_every_batch_operation_without_a_declared_length() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for path in BODY_CARRYING_ENTITY_OPERATIONS {
        let response = post_without_declared_length(client, path).await;
        assert_eq!(response.status, StatusCode::LENGTH_REQUIRED, "POST {path}: {:?}", response.body);
        assert!(response.body.is_empty(), "POST {path} carried a body on a 411: {:?}", response.body);
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.13.2 creates a hosted `@context` with an HTTP POST carrying a
/// payload body, so clause 6.3.4's `Content-Length` precondition holds for it
/// as for any other body-carrying operation.
#[tokio::test]
async fn regression_length_required_creating_a_jsonld_context_without_a_declared_length() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let response = post_without_declared_length(client, "jsonldContexts").await;

    assert_eq!(response.status, StatusCode::LENGTH_REQUIRED, "{:?}", response.body);
    assert!(response.body.is_empty(), "clause 6.3.4 forbids a payload body on a 411");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.4 exempts only an *absent* `Content-Length` from a payload body.
/// A declared length of zero is a present header describing an empty payload,
/// which clause 5.5.4 refuses as `InvalidRequest` because an empty body "is not
/// a valid JSON document". Every family of body extractor must agree on that,
/// so the single-entity, batch and hosted-`@context` paths are asserted
/// together.
#[tokio::test]
async fn regression_length_required_a_declared_zero_length_stays_an_invalid_request() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for path in ["entities", "entityOperations/create", "entityOperations/delete", "jsonldContexts"] {
        let response = post_declaring_zero_length(client, path).await;
        assert_eq!(response.status, StatusCode::BAD_REQUEST, "POST {path}: {:?}", response.body);
        let problem = response.json_value().expect("an InvalidRequest carries a problem details body");
        assert_eq!(problem["type"], json!(INVALID_REQUEST), "POST {path}: {problem}");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.4 lists the acceptable media types as `application/json`,
/// `application/ld+json` and `application/geo+json` and then says: "The order of
/// the list above is significant. If the Accept header can be expanded to more
/// than one of the options of the list, the first one of the list shall be
/// selected, unless amended by the HTTP Accept header processing rules, e.g. the
/// presence of a "q" parameter indicating a relative weight".
///
/// Negotiation broke equal-weight ties by the order the client wrote its
/// entries in, so `application/ld+json, application/json` answered `ld+json`
/// where the clause requires `json`. Neither writing order may decide it.
#[tokio::test]
async fn regression_accept_equal_weights_are_broken_by_the_clause_order() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for accept in [
        "application/ld+json, application/json",
        "application/json, application/ld+json",
        "application/geo+json, application/ld+json, application/json",
    ] {
        let response = get_accepting(client, "entities?type=Building", accept).await;
        assert_eq!(response.status, StatusCode::OK, "Accept: {accept}: {:?}", response.body);
        assert_eq!(
            content_type(&response),
            Some("application/json"),
            "Accept: {accept} must negotiate to the first representation the clause lists"
        );
    }

    // The clause's own exception: a weight amends the order, so a heavier entry
    // still wins wherever it sits in the list.
    let weighted = get_accepting(client, "entities?type=Building", "application/json;q=0.5, application/ld+json").await;
    assert_eq!(weighted.status, StatusCode::OK, "{:?}", weighted.body);
    assert_eq!(content_type(&weighted), Some("application/ld+json"));

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.4: the `Accept` header "shall include (or define a media range
/// that can be expanded to) at least one of" the three NGSI-LD media types.
/// `application/*` is such a media range and covers all three, so it is
/// acceptable and expands to the first the clause lists; the broker recognised
/// only `*/*` and answered a 406.
#[tokio::test]
async fn regression_accept_an_application_media_range_is_acceptable() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for accept in ["application/*", "*/*"] {
        let response = get_accepting(client, "entities?type=Building", accept).await;
        assert_eq!(response.status, StatusCode::OK, "Accept: {accept}: {:?}", response.body);
        assert_eq!(
            content_type(&response),
            Some("application/json"),
            "Accept: {accept} expands to the first representation the clause lists"
        );
    }

    // A range under another top-level type covers none of the three and is
    // still the clause 6.3.4 refusal.
    let refused = get_accepting(client, "entities?type=Building", "text/*").await;
    assert_eq!(refused.status, StatusCode::NOT_ACCEPTABLE, "{:?}", refused.body);

    scope.finish().await.expect("cleanup failed");
}
