//! An entity a Context Source answers with is held to the attribute rules a
//! write to this broker is held to.
//!
//! A client writing a `unitCode` that names no UN/CEFACT common code (ETSI GS
//! CIM 009 v1.9.1 Table 5.2.5-1), a `unitCode` on a family clause 4.5.2 leaves
//! unitless, or a `GeoProperty` that is no RFC 7946 geometry (clause 4.7) is
//! answered `BadRequestData`. A Context Source answering with the same
//! attributes had them relayed verbatim: entity decoding keeps an attribute it
//! cannot read as raw JSON rather than failing the entity, and the temporal
//! merge never decoded the source's answer at all. So the hub handed its
//! client, on a query, exactly what it refuses to hold.
//!
//! Both the current and the temporal read now drop such an entity from the
//! source's answer with a clause 6.3.17 `RevalidationFailed` warning naming the
//! source, the entity and the refused attribute, and merge the rest of the
//! answer as usual.
//!
//! Each scenario releases its registration and its mock source before it
//! asserts anything, because a registration left behind points at a port that
//! no longer listens.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::context_source_adapter::ContextSourceAdapter;
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;

/// The resource a forwarded current-entity query lands on at the source.
const FORWARDED_QUERY_PATH: &str = "/ngsi-ld/v1/entities";

/// The resource a forwarded temporal query lands on at the source.
const FORWARDED_TEMPORAL_QUERY_PATH: &str = "/ngsi-ld/v1/temporal/entities";

/// Register `source` inclusively for `operations` over `entity_type`,
/// answering the registration status.
async fn register(client: &TestClient, scope: &TestEnvironment, source: &ContextSourceAdapter, entity_type: &str, operations: &[&str]) -> StatusCode {
    let registration = json!({
        "id": scope.id(&format!("registration:{entity_type}")),
        "type": "ContextSourceRegistration",
        "endpoint": source.url("/"),
        "mode": "inclusive",
        "operations": operations,
        "information": [{"entities": [{"type": entity_type}]}],
        "@context": [TEST_SUITE_CONTEXT]
    });
    client.create_registration(&registration).await.expect("create registration").status
}

/// A read of `path` with the test suite context.
async fn read(client: &TestClient, path: &str) -> TestResponse {
    client
        .raw(Method::GET, path)
        .header("Accept", "application/json")
        .header(
            "Link",
            format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"").as_str(),
        )
        .send()
        .await
        .expect("read request")
}

/// The identifiers of the entities a query answered, in answer order.
fn answered_ids(response: &TestResponse) -> Vec<String> {
    response
        .json_value()
        .expect("a JSON page")
        .as_array()
        .expect("a query answers an array")
        .iter()
        .map(|entity| entity["id"].as_str().expect("an id").to_owned())
        .collect()
}

/// The `NGSILD-Warning` values of `response`.
fn warnings(response: &TestResponse) -> Vec<String> {
    response
        .headers
        .get_all("ngsild-warning")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(str::to_owned)
        .collect()
}

/// Assert `warnings` report `entity` as a clause 6.3.17 revalidation failure
/// whose detail names `detail`.
fn assert_dropped(warnings: &[String], entity: &str, detail: &str) {
    assert!(
        warnings
            .iter()
            .any(|warning| warning.starts_with("111") && warning.contains(entity) && warning.contains(detail)),
        "{entity} is reported as dropped for {detail:?}: {warnings:?}"
    );
}

/// Clause 5.7.2.4 with clause 6.3.17: of a source's current entities, the ones
/// carrying a `unitCode` that is no common code, a `unitCode` on a
/// Relationship, or an open Polygon ring are dropped, each with a warning, and
/// the admissible one is merged.
#[tokio::test]
async fn regression_remote_revalidation_a_query_drops_the_source_entities_a_write_would_refuse() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("RevalidatedProbe");
    let valid = scope.id("RevalidatedProbe:valid");
    let not_a_code = scope.id("RevalidatedProbe:not-a-code");
    let unitless = scope.id("RevalidatedProbe:unitless");
    let open_ring = scope.id("RevalidatedProbe:open-ring");
    let source = ContextSourceAdapter::start().await;
    let answer = json!([
        {"id": valid, "type": entity_type, "speed": {"type": "Property", "value": 50, "unitCode": "KMH"}, "@context": TEST_SUITE_CONTEXT.as_str()},
        {"id": not_a_code, "type": entity_type, "speed": {"type": "Property", "value": 50, "unitCode": "km/h"}, "@context": TEST_SUITE_CONTEXT.as_str()},
        {
            "id": unitless,
            "type": entity_type,
            "parkedIn": {"type": "Relationship", "object": "urn:ngsi-ld:Parking:1", "unitCode": "KGM"},
            "@context": TEST_SUITE_CONTEXT.as_str()
        },
        {
            "id": open_ring,
            "type": entity_type,
            "location": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1]]]}},
            "@context": TEST_SUITE_CONTEXT.as_str()
        }
    ]);
    source.set_stub_with_body(Method::GET, FORWARDED_QUERY_PATH, 200, &answer).await;
    let (registered, queried) = {
        let client = scope.client();
        let registered = register(client, &scope, &source, &entity_type, &["federationOps"]).await;
        let queried = read(client, &format!("entities?type={entity_type}")).await;
        (registered, queried)
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    assert_eq!(registered, StatusCode::CREATED);
    assert_eq!(queried.status, StatusCode::OK, "{:?}", queried.body);
    assert_eq!(answered_ids(&queried), vec![valid], "only the admissible source entity is merged");
    let warnings = warnings(&queried);
    assert_dropped(&warnings, &not_a_code, "attribute 'speed': 'unitCode' is not a UN/CEFACT common code");
    assert_dropped(&warnings, &unitless, "attribute 'parkedIn': a Relationship carries no 'unitCode'");
    assert_dropped(&warnings, &open_ring, "GeoJSON linear ring is open");
}

/// Clause 5.7.4.4 with clause 6.3.17: a source's temporal entities are held to
/// the same rules, instance by instance, in the normalized and in the
/// `temporalValues` representation, where they used to be merged as raw JSON.
#[tokio::test]
async fn regression_remote_revalidation_a_temporal_query_drops_the_source_entities_a_write_would_refuse() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let entity_type = scope.entity_type("RevalidatedHistory");
    let valid = scope.id("RevalidatedHistory:valid");
    let not_a_code = scope.id("RevalidatedHistory:not-a-code");
    let past_pole = scope.id("RevalidatedHistory:past-pole");
    let paired = scope.id("RevalidatedHistory:paired");
    let source = ContextSourceAdapter::start().await;
    let answer = json!([
        {
            "id": valid,
            "type": entity_type,
            "speed": [{"type": "Property", "value": 50, "unitCode": "KMH", "observedAt": "2020-01-01T00:00:00Z"}],
            "@context": TEST_SUITE_CONTEXT.as_str()
        },
        {
            "id": not_a_code,
            "type": entity_type,
            "speed": [
                {"type": "Property", "value": 50, "unitCode": "KMH", "observedAt": "2020-01-01T00:00:00Z"},
                {"type": "Property", "value": 51, "unitCode": "km/h", "observedAt": "2020-01-02T00:00:00Z"}
            ],
            "@context": TEST_SUITE_CONTEXT.as_str()
        },
        {
            "id": past_pole,
            "type": entity_type,
            "location": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0, 91]}, "observedAt": "2020-01-01T00:00:00Z"}],
            "@context": TEST_SUITE_CONTEXT.as_str()
        },
        {
            "id": paired,
            "type": entity_type,
            "location": {"type": "GeoProperty", "values": [[{"type": "Point", "coordinates": [181, 0]}, "2020-01-01T00:00:00Z"]]},
            "@context": TEST_SUITE_CONTEXT.as_str()
        }
    ]);
    source.set_stub_with_body(Method::GET, FORWARDED_TEMPORAL_QUERY_PATH, 200, &answer).await;
    let (registered, queried) = {
        let client = scope.client();
        let registered = register(client, &scope, &source, &entity_type, &["queryTemporal"]).await;
        let queried = read(
            client,
            &format!("temporal/entities?type={entity_type}&timerel=after&timeAt=2019-01-01T00:00:00Z"),
        )
        .await;
        (registered, queried)
    };
    source.shutdown().await;
    scope.finish().await.expect("cleanup failed");

    assert_eq!(registered, StatusCode::CREATED);
    assert_eq!(queried.status, StatusCode::OK, "{:?}", queried.body);
    assert_eq!(answered_ids(&queried), vec![valid], "only the admissible source history is merged");
    let warnings = warnings(&queried);
    assert_dropped(&warnings, &not_a_code, "attribute 'speed': 'unitCode' is not a UN/CEFACT common code");
    assert_dropped(&warnings, &past_pole, "latitude 91 is outside [-90, 90]");
    assert_dropped(&warnings, &paired, "longitude 181 is outside [-180, 180]");
}
