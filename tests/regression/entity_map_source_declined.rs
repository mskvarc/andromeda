//! Relaying a registered Context Source's 501 out of an `EntityMap` creation.
//!
//! ETSI GS CIM 009 v1.9.1 Table 6.34.3.1-1 lists, among the responses of
//! "Create `EntityMap` for Query Entities", a `ProblemDetails` body with
//! "501 Not Implemented -- It is used by Registered Context Sources to indicate
//! that the data format of the request is unsupported see clause 6.3.7". A
//! source's 501 is relayed as the operation's answer rather than collapsed, with
//! every other non-404 source answer, into a clause 6.3.17 warning string on a
//! 201.
//!
//! The relay is the one Table 6.4.3.2-3 already gives the entity reads: it
//! applies only where a single exclusive or redirect registration holds all of
//! the data, because only then is the source's answer the operation's answer.
//!
//! Every scenario here registers a Context Source, so every scenario releases
//! its registrations and its mock sources before it asserts anything: a
//! registration left behind by a failed assertion points at a mock port that is
//! no longer listening, and the broker goes on forwarding to it for every later
//! request that matches.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::context_source_adapter::ContextSourceAdapter;
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail::OPERATION_NOT_SUPPORTED};
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The resource a forwarded `EntityMap` creation lands on at a Context Source.
const FORWARDED_MAP_PATH: &str = "/ngsi-ld/v1/entityMaps";
/// The temporal twin of [`FORWARDED_MAP_PATH`].
const FORWARDED_TEMPORAL_MAP_PATH: &str = "/ngsi-ld/v1/temporal/entityMaps";

/// What one creation attempt produced, kept so every assertion runs after the
/// registrations and the mock sources are gone.
struct Attempt {
    /// The status each registration creation answered with, in order.
    registrations: Vec<StatusCode>,
    /// How many forwarded creations each mock source received, in order.
    forwarded: Vec<usize>,
    /// The creation response itself.
    response: TestResponse,
}

impl Attempt {
    /// Assert every registration was accepted and every source was contacted
    /// exactly once, which is what makes the response assertions meaningful.
    fn assert_setup_held(&self) {
        for status in &self.registrations {
            assert_eq!(*status, StatusCode::CREATED, "the registration is accepted");
        }
        for count in &self.forwarded {
            assert_eq!(*count, 1, "the creation is forwarded to the registered source");
        }
    }

    /// The response body as text, for assertion messages.
    fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.response.body).into_owned()
    }

    /// Assert the creation answered `expected`.
    fn assert_status(&self, expected: StatusCode, reason: &str) {
        assert_eq!(self.response.status, expected, "{reason}: {}", self.body_text());
    }

    /// Assert the response is a `ProblemDetails` body of the given error type.
    fn assert_problem_type(&self, expected: &str) {
        let body: Value = self.response.json_value().expect("a ProblemDetails body");
        assert_eq!(body.get("type").and_then(Value::as_str), Some(expected), "{body}");
        assert!(body.get("title").is_some(), "clause 5.5.3 requires a title: {body}");
        assert!(body.get("detail").is_some(), "clause 6.3.2 asks the detail to say more: {body}");
    }

    /// Assert the response carries the clause 6.3.17 warning header.
    fn assert_warned(&self) {
        assert!(
            self.response.headers.contains_key("ngsild-warning"),
            "the declining source is reported as a clause 6.3.17 warning: {:?}",
            self.response.headers
        );
    }
}

/// A registration covering one Entity type, under `mode`.
fn registration_for_type(id: &str, endpoint: &str, mode: &str, entity_type: &str, operations: &[&str]) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": endpoint,
        "mode": mode,
        "operations": operations,
        "information": [{"entities": [{"type": entity_type}]}],
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Register `body` and report the status without asserting: an assertion here
/// would abandon the registrations already created.
async fn register(client: &TestClient, body: &Value) -> StatusCode {
    client.create_registration(body).await.expect("create registration").status
}

/// The `EntityMap` a source that does answer returns.
fn remote_map() -> Value {
    json!({
        "id": "urn:ngsi-ld:EntityMap:remote-source",
        "type": "EntityMap",
        "entityMap": {},
        "linkedMaps": {}
    })
}

/// The JSON-LD context `Link` header the creation request carries.
fn context_link() -> String {
    format!(
        "<{}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"",
        TEST_SUITE_CONTEXT.as_str()
    )
}

/// Create an `EntityMap` through the `GET /entityMaps` binding.
async fn create_map(client: &TestClient, query: &str) -> TestResponse {
    client
        .raw(Method::GET, &format!("/ngsi-ld/v1/entityMaps?{query}"))
        .header("link", context_link().as_str())
        .header("accept", "application/ld+json")
        .send()
        .await
        .expect("entity map creation request")
}

/// Table 6.34.3.1-1: the 501 of the one redirect source holding all of the data
/// is the operation's answer, carried as a `ProblemDetails` body.
#[tokio::test]
async fn regression_entity_map_declined_a_lone_redirect_source_501_is_relayed() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("MapDeclinedRedirectProbe");
    let source = ContextSourceAdapter::start().await;

    let attempt = {
        let client = scope.client();
        let registration = register(
            client,
            &registration_for_type(
                &scope.id("registration:map-declined-redirect"),
                &source.url("/"),
                "redirect",
                &entity_type,
                &["redirectionOps"],
            ),
        )
        .await;
        source.set_stub(Method::GET, FORWARDED_MAP_PATH, 501).await;
        let response = create_map(client, &format!("type={entity_type}")).await;
        Attempt {
            registrations: vec![registration],
            forwarded: vec![source.get_count(Method::GET, FORWARDED_MAP_PATH)],
            response,
        }
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    attempt.assert_setup_held();
    attempt.assert_status(StatusCode::NOT_IMPLEMENTED, "the source's 501 is relayed");
    attempt.assert_problem_type(OPERATION_NOT_SUPPORTED.as_str());
}

/// The POST binding of clause 6.34.3.2 performs the same operation, so it
/// relays the same answer.
#[tokio::test]
async fn regression_entity_map_declined_the_post_binding_relays_the_same_501() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("MapDeclinedPostProbe");
    let source = ContextSourceAdapter::start().await;

    let attempt = {
        let client = scope.client();
        let registration = register(
            client,
            &registration_for_type(
                &scope.id("registration:map-declined-post"),
                &source.url("/"),
                "redirect",
                &entity_type,
                &["redirectionOps"],
            ),
        )
        .await;
        source.set_stub(Method::GET, FORWARDED_MAP_PATH, 501).await;
        let response = client
            .raw(Method::POST, "/ngsi-ld/v1/entityMaps")
            .header("content-type", "application/ld+json")
            .header("accept", "application/ld+json")
            .json(&json!({"type": "Query", "entities": [{"type": entity_type}], "@context": TEST_SUITE_CONTEXT.as_str()}))
            .send()
            .await
            .expect("entity map creation post");
        Attempt {
            registrations: vec![registration],
            forwarded: vec![source.get_count(Method::GET, FORWARDED_MAP_PATH)],
            response,
        }
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    attempt.assert_setup_held();
    attempt.assert_status(StatusCode::NOT_IMPLEMENTED, "the POST binding relays the same 501");
    attempt.assert_problem_type(OPERATION_NOT_SUPPORTED.as_str());
}

/// The relay keeps the bound Table 6.4.3.2-3 puts on the same answer: with more
/// than one matching registration the declining source does not hold all of the
/// data, so its 501 stays a clause 6.3.17 warning and the map is created.
#[tokio::test]
async fn regression_entity_map_declined_a_501_among_several_sources_is_not_relayed() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("MapDeclinedSeveralProbe");
    let declining = ContextSourceAdapter::start().await;
    let answering = ContextSourceAdapter::start().await;

    let attempt = {
        let client = scope.client();
        let first = register(
            client,
            &registration_for_type(
                &scope.id("registration:map-declined-several-1"),
                &declining.url("/"),
                "redirect",
                &entity_type,
                &["redirectionOps"],
            ),
        )
        .await;
        let second = register(
            client,
            &registration_for_type(
                &scope.id("registration:map-declined-several-2"),
                &answering.url("/"),
                "inclusive",
                &entity_type,
                &["federationOps"],
            ),
        )
        .await;
        declining.set_stub(Method::GET, FORWARDED_MAP_PATH, 501).await;
        answering.set_stub_with_body(Method::GET, FORWARDED_MAP_PATH, 201, &remote_map()).await;
        let response = create_map(client, &format!("type={entity_type}")).await;
        Attempt {
            registrations: vec![first, second],
            forwarded: vec![
                declining.get_count(Method::GET, FORWARDED_MAP_PATH),
                answering.get_count(Method::GET, FORWARDED_MAP_PATH),
            ],
            response,
        }
    };
    declining.shutdown().await;
    answering.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    attempt.assert_setup_held();
    attempt.assert_status(StatusCode::CREATED, "a 501 from one of several sources does not fail the creation");
    attempt.assert_warned();
}

/// An inclusive registration never holds all of the data on its own -- clause
/// 4.3.6.2 lets the broker hold context data about the same Entities itself --
/// so its 501 is a warning even when it is the only registration that matched.
#[tokio::test]
async fn regression_entity_map_declined_a_lone_inclusive_source_501_is_only_a_warning() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("MapDeclinedInclusiveProbe");
    let source = ContextSourceAdapter::start().await;

    let attempt = {
        let client = scope.client();
        let registration = register(
            client,
            &registration_for_type(
                &scope.id("registration:map-declined-inclusive"),
                &source.url("/"),
                "inclusive",
                &entity_type,
                &["federationOps"],
            ),
        )
        .await;
        source.set_stub(Method::GET, FORWARDED_MAP_PATH, 501).await;
        let response = create_map(client, &format!("type={entity_type}")).await;
        Attempt {
            registrations: vec![registration],
            forwarded: vec![source.get_count(Method::GET, FORWARDED_MAP_PATH)],
            response,
        }
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    attempt.assert_setup_held();
    attempt.assert_status(StatusCode::CREATED, "an inclusive source declining does not fail the creation");
    attempt.assert_warned();
}

/// Only the 501 of clause 6.3.7 is relayed. Every other status a source answers
/// with stays what it was: a clause 6.3.17 warning beside a created map.
#[tokio::test]
async fn regression_entity_map_declined_a_non_501_error_still_creates_the_map() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("MapDeclinedNon501Probe");
    let source = ContextSourceAdapter::start().await;

    let attempt = {
        let client = scope.client();
        let registration = register(
            client,
            &registration_for_type(
                &scope.id("registration:map-declined-non-501"),
                &source.url("/"),
                "redirect",
                &entity_type,
                &["redirectionOps"],
            ),
        )
        .await;
        source.set_stub(Method::GET, FORWARDED_MAP_PATH, 500).await;
        let response = create_map(client, &format!("type={entity_type}")).await;
        Attempt {
            registrations: vec![registration],
            forwarded: vec![source.get_count(Method::GET, FORWARDED_MAP_PATH)],
            response,
        }
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    attempt.assert_setup_held();
    attempt.assert_status(StatusCode::CREATED, "a 500 is not the answer Table 6.34.3.1-1 relays");
    attempt.assert_warned();
}

/// A source that answers the creation normally is unaffected: the map is
/// created, folded with the remote one, and answered 201.
#[tokio::test]
async fn regression_entity_map_declined_a_source_that_answers_still_yields_201() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("MapDeclinedHappyProbe");
    let source = ContextSourceAdapter::start().await;

    let attempt = {
        let client = scope.client();
        let registration = register(
            client,
            &registration_for_type(
                &scope.id("registration:map-declined-happy"),
                &source.url("/"),
                "redirect",
                &entity_type,
                &["redirectionOps"],
            ),
        )
        .await;
        source.set_stub_with_body(Method::GET, FORWARDED_MAP_PATH, 201, &remote_map()).await;
        let response = create_map(client, &format!("type={entity_type}")).await;
        Attempt {
            registrations: vec![registration],
            forwarded: vec![source.get_count(Method::GET, FORWARDED_MAP_PATH)],
            response,
        }
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    attempt.assert_setup_held();
    attempt.assert_status(StatusCode::CREATED, "a source that answers leaves the creation successful");
    assert!(
        attempt.response.headers.contains_key("ngsild-entitymap"),
        "the created map is named in the NGSILD-EntityMap header"
    );
}

/// The temporal creation resource of clause 5.14.5 forwards through the same
/// path, so a lone proxied source declining there is relayed the same way.
/// Table 6.35.3.1-1 lists no 501 row, but it lists no 203 row either while
/// clause 4.3.6.8 mandates one, so its silence is not a prohibition; answering
/// 201 with a map that omits the source holding all of the data would be.
#[tokio::test]
async fn regression_entity_map_declined_the_temporal_resource_relays_it_too() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("MapDeclinedTemporalProbe");
    let source = ContextSourceAdapter::start().await;

    let attempt = {
        let client = scope.client();
        let registration = register(
            client,
            &registration_for_type(
                &scope.id("registration:map-declined-temporal"),
                &source.url("/"),
                "redirect",
                &entity_type,
                &["createEntityMapQueryTemporal"],
            ),
        )
        .await;
        source.set_stub(Method::GET, FORWARDED_TEMPORAL_MAP_PATH, 501).await;
        let response = client
            .raw(
                Method::GET,
                &format!("/ngsi-ld/v1/temporal/entityMaps?type={entity_type}&timerel=after&timeAt=2020-01-01T00:00:00Z"),
            )
            .header("link", context_link().as_str())
            .header("accept", "application/ld+json")
            .send()
            .await
            .expect("temporal entity map creation request");
        Attempt {
            registrations: vec![registration],
            forwarded: vec![source.get_count(Method::GET, FORWARDED_TEMPORAL_MAP_PATH)],
            response,
        }
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    attempt.assert_setup_held();
    attempt.assert_status(StatusCode::NOT_IMPLEMENTED, "the temporal creation relays the same 501");
    attempt.assert_problem_type(OPERATION_NOT_SUPPORTED.as_str());
}
