//! Which refusals clause 6.3.20 answers with `InvalidRequest`.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.20: "If an HTTP request for an operation
//! contains parameters that are incompatible with the operation, or it contains
//! values of the options parameter that are not supported by the operation, an
//! HTTP error response of type `InvalidRequest` should be returned." Tables
//! 6.3.7-1 and 6.3.12-1 make `format` and `options` two spellings of one
//! choice -- `options` is "an alternative mechanism to include the format
//! parameter. Deprecated" -- so an unsupported value of either is the same
//! failure, and Table 6.3.7-1 fixes the entity renderings while Table 6.3.12-1
//! fixes the temporal ones.
//!
//! The broker answered `BadRequestData` instead, because the two parameters
//! were parsed inside `serde_urlencoded`, whose failures carry only a message:
//! the binding then guessed the error type from that message and guessed
//! literals no deserialiser emitted. The parameters now deserialise as the
//! request wrote them and are refused by the parameter set's validation, where
//! the error type is typed rather than read out of a string.
//!
//! `BadRequestData` remains the answer for a value *inside* a parameter the
//! operation does define -- clause 5.5.2 gives it to "input data which does not
//! meet the requirements of the operation" -- which is why the incomplete
//! geoquery of clause 4.10 and a non-numeric `limit` are asserted here too.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The problem type clause 6.3.20 names for an unsupported parameter value.
const INVALID_REQUEST: &str = "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest";

/// The problem type clause 5.5.2 names for input the operation cannot accept.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The temporal window Table 6.3.12-1's operations need beside the parameter
/// under test (clause 4.11).
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z";

/// One Vehicle, so the operations under test address something real.
fn vehicle(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 60, "observedAt": "2025-01-01T00:00:00Z"},
        "@context": [CORE_CONTEXT]
    })
}

/// Create the entity the reads address.
async fn create_entity(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Send one read against a root-relative path.
async fn read(client: &TestClient, path: &str) -> TestResponse {
    client
        .raw(Method::GET, path)
        .header("Accept", "application/ld+json")
        .send()
        .await
        .expect("read request")
}

/// Assert a refusal is a 400 carrying `expected_type`.
fn assert_refused_as(response: &TestResponse, expected_type: &str, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(expected_type), "{scenario}: {problem}");
    assert!(problem["title"].is_string(), "{scenario}: a problem carries a title: {problem}");
}

/// Every unsupported `format` and `options` value Table 6.3.7-1 leaves out is
/// a clause 6.3.20 `InvalidRequest`, on the retrieval and on the query alike,
/// and so is a value supplied alongside a supported one.
#[tokio::test]
async fn regression_invalid_parameters_an_unsupported_entity_representation_is_an_invalid_request() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:invalid-representation");
    create_entity(client, &vehicle(&id)).await;

    for parameters in [
        "format=invalidFormat",
        "format=",
        "format=temporalValues",
        "format=aggregatedValues",
        "options=invalidOptions",
        "options=keyValues,invalid",
        "options=invalid,keyValues",
        "options=",
        "options=normalized,concise",
        "options=sysAttrs,sysAttrs",
        // Table 6.3.12-1's tokens are not Table 6.3.7-1's.
        "options=temporalValues",
        // A supported `format` does not excuse an `options` the operation
        // cannot act on: clause 6.3.20 reads both.
        "format=keyValues&options=invalid",
    ] {
        let retrieved = read(client, &format!("entities/{id}?{parameters}")).await;
        assert_refused_as(&retrieved, INVALID_REQUEST, &format!("retrieve with {parameters}"));

        let queried = read(client, &format!("entities?type=Vehicle&{parameters}")).await;
        assert_refused_as(&queried, INVALID_REQUEST, &format!("query with {parameters}"));
    }

    scope.finish().await.expect("cleanup failed");
}

/// Table 6.3.12-1: the temporal `format` "shall be one of: temporalValues,
/// aggregatedValues", and its `options` carries those two tokens and
/// `sysAttrs`. Everything else is the same clause 6.3.20 failure.
#[tokio::test]
async fn regression_invalid_parameters_an_unsupported_temporal_representation_is_an_invalid_request() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:invalid-temporal-representation");
    create_entity(client, &vehicle(&id)).await;

    for parameters in [
        "format=invalidFormat",
        // Table 6.3.12-1 admits neither the normalized temporal representation
        // as a value nor any entity rendering.
        "format=normalized",
        "format=keyValues",
        "format=concise",
        "options=invalidOptions",
        "options=temporalValues,invalidOptions",
        "options=keyValues",
        "options=temporalValues,aggregatedValues",
        "format=temporalValues&options=aggregatedValues,invalidOptions",
    ] {
        let retrieved = read(client, &format!("temporal/entities/{id}?{WINDOW}&{parameters}")).await;
        assert_refused_as(&retrieved, INVALID_REQUEST, &format!("temporal retrieve with {parameters}"));

        let queried = read(client, &format!("temporal/entities?type=Vehicle&{WINDOW}&{parameters}")).await;
        assert_refused_as(&queried, INVALID_REQUEST, &format!("temporal query with {parameters}"));
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.20's other half: "parameters that are incompatible with the
/// operation". A parameter no table of the operation defines is one, whichever
/// operation it is sent to.
#[tokio::test]
async fn regression_invalid_parameters_a_parameter_the_operation_does_not_define_is_an_invalid_request() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:unknown-parameter");
    create_entity(client, &vehicle(&id)).await;

    for path in [
        format!("entities/{id}?bogusParameter=1"),
        format!("entities/{id}?attrs=speed&bogusParameter=1"),
        "entities?type=Vehicle&bogusParameter=1".to_owned(),
        format!("temporal/entities/{id}?{WINDOW}&bogusParameter=1"),
        format!("temporal/entities?type=Vehicle&{WINDOW}&bogusParameter=1"),
        "csourceRegistrations?type=Vehicle&bogusParameter=1".to_owned(),
        "subscriptions?bogusParameter=1".to_owned(),
    ] {
        let response = read(client, &path).await;
        assert_refused_as(&response, INVALID_REQUEST, &format!("read {path}"));
    }

    scope.finish().await.expect("cleanup failed");
}

/// The supported values stay supported: Table 6.3.7-1's four spellings of three
/// renderings, Table 6.3.12-1's two temporal ones, and the `sysAttrs` token of
/// clause 6.3.11 beside each of them.
#[tokio::test]
async fn regression_invalid_parameters_every_representation_the_tables_define_is_accepted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:supported-representation");
    create_entity(client, &vehicle(&id)).await;

    for parameters in [
        "format=normalized",
        "format=concise",
        "format=simplified",
        "format=keyValues",
        "options=normalized",
        "options=concise",
        "options=simplified",
        "options=keyValues",
        "options=sysAttrs",
        "options=keyValues,sysAttrs",
    ] {
        let retrieved = read(client, &format!("entities/{id}?{parameters}")).await;
        assert_eq!(retrieved.status, StatusCode::OK, "retrieve with {parameters}: {:?}", retrieved.body);
    }

    for parameters in [
        "format=temporalValues",
        "options=temporalValues",
        "options=sysAttrs",
        "options=temporalValues,sysAttrs",
    ] {
        let retrieved = read(client, &format!("temporal/entities/{id}?{WINDOW}&{parameters}")).await;
        assert_eq!(retrieved.status, StatusCode::OK, "temporal retrieve with {parameters}: {:?}", retrieved.body);
    }

    scope.finish().await.expect("cleanup failed");
}

/// A value inside a parameter the operation does define is not clause 6.3.20's
/// failure: clause 5.5.2 gives "input data which does not meet the requirements
/// of the operation" to `BadRequestData`, and clauses 5.7.2.4 and 5.10.2.4 say
/// so of a geoquery that is not valid as per clause 4.10.
#[tokio::test]
async fn regression_invalid_parameters_a_value_inside_a_defined_parameter_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:defined-parameter-value");
    create_entity(client, &vehicle(&id)).await;

    for parameters in [
        "type=Vehicle&limit=notANumber",
        // Clause 4.10 needs all three of georel, geometry and coordinates.
        "type=Vehicle&geometry=Point&coordinates=%5B1,2%5D",
        "type=Vehicle&georel=near%3BmaxDistance%3D%3D100",
        "type=Vehicle&idPattern=%5B",
    ] {
        let queried = read(client, &format!("entities?{parameters}")).await;
        assert_refused_as(&queried, BAD_REQUEST_DATA, &format!("query with {parameters}"));
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.10 spells a `near` distance as a `PositiveNumber` -- an IETF RFC 8259
/// JSON number that is strictly positive. A distance outside that grammar makes
/// the whole geoquery "not valid as per clause 4.10", which clause 5.7.2.4 maps
/// to `BadRequestData`. Every case below carries a well-formed `geometry` and
/// `coordinates`, so the distance spelling is the only thing under test: a
/// laxer parser that accepted `f64`'s own `+500` / `.5` / `007` spellings would
/// answer `200` here.
#[tokio::test]
async fn regression_invalid_parameters_a_near_distance_outside_the_positive_number_grammar_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:near-distance-grammar");
    create_entity(client, &vehicle(&id)).await;

    // `near;maxDistance==<distance>` over a valid Point, with the `;`, `==`, and
    // any `+`/`,` percent-encoded so the server sees them verbatim.
    let geoquery = |distance: &str| format!("type=Vehicle&geometry=Point&coordinates=%5B1,2%5D&georel=near%3BmaxDistance%3D%3D{distance}");

    // A well-formed positive distance is accepted (the entity carries no
    // location, so the page is simply empty).
    let accepted = read(client, &format!("entities?{}", geoquery("500"))).await;
    assert_eq!(accepted.status, StatusCode::OK, "a well-formed distance is accepted: {:?}", accepted.body);

    for distance in [
        "%2B500",  // +500 -- a leading sign is not an RFC 8259 number
        ".5",      // a bare leading decimal point
        "5.",      // a bare trailing decimal point
        "007",     // a leading zero
        "1%2C000", // 1,000 -- a thousands separator
        "0",       // not strictly positive
        "-5",      // negative
        "NaN",     // not a number at all
    ] {
        let queried = read(client, &format!("entities?{}", geoquery(distance))).await;
        assert_refused_as(&queried, BAD_REQUEST_DATA, &format!("near distance {distance}"));
    }

    scope.finish().await.expect("cleanup failed");
}
