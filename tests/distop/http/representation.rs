//! Representation and format negotiation (clause 6.3.7, 6.3.8, 4.5.16): the
//! simplified `keyValues` form, the normalized default forwarded to sources, the
//! `GeoJSON` `Feature`/`FeatureCollection` forms, the `501` on an unsupported
//! forwarded representation, the `203 Preference-Applied` version-amendment
//! responses driven by a `Prefer: ngsi-ld=<version>` request header, and the
//! profile `Link` version hint of clause 6.3.21.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, vehicle},
    http::forwarding::{FORWARDED_ENTITIES_PATH, context_link, context_link_header, forwarded_entity_path, retrieval_source},
    oracle::{assert_status, json_body},
    topology::{self, BrokerRole},
};
use pretty_assertions::assert_eq;
use reqwest::Method;
use serde_json::json;
use std::time::Duration;

/// The `Prefer` header asking for the previous NGSI-LD version (clause 4.3.6.8
/// names versions as `major.minor`), so the broker has a fallback to apply.
const PREFER_PREVIOUS_VERSION: &str = "ngsi-ld=1.8";

/// The RFC 6906 profile `Link` naming the previous NGSI-LD version in the
/// `<https://uri.etsi.org/ngsi-ld/profile/version>` form of clause 6.3.21.
const PROFILE_LINK_PREVIOUS_VERSION: &str = "<https://uri.etsi.org/ngsi-ld/profile/1.8>; rel=\"profile\"";

/// A location `GeoProperty` node for a `Vehicle`, so `GeoJSON` rendering has a
/// geometry to project.
fn located_vehicle(id: &str) -> serde_json::Value {
    entity(
        id,
        "Vehicle",
        &[
            ("speed", property(json!(42))),
            (
                "location",
                json!({ "type": "GeoProperty", "value": { "type": "Point", "coordinates": [13.4, 52.5] } }),
            ),
        ],
    )
}

/// `options=keyValues` returns the simplified representation, collapsing each
/// attribute to its value (clause 4.5.16). Confirms REQ-HTTP-1.
#[tokio::test]
async fn dist_http_query_format_key_values_returns_simplified_representation() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:kv");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 42))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=Vehicle&options=keyValues", "application/ld+json", Some(context_link()))
        .await
        .expect("keyValues query");
    assert_status(&response, 200);
    let entities = json_body(&response);
    let found = entities
        .as_array()
        .unwrap_or_else(|| panic!("query returns an array"))
        .iter()
        .find(|entity| entity["id"].as_str() == Some(entity_id.as_str()));
    let found = found.unwrap_or_else(|| panic!("the created vehicle is returned"));
    assert_eq!(found["speed"].as_i64(), Some(42), "keyValues collapses the attribute to its raw value");

    topology.finish().await.expect("topology cleanup");
}

/// A forwarded query defaults to the normalized representation so results remain
/// mergeable, even when the client asked for `keyValues` (clause 6.3.8).
/// Confirms REQ-HTTP-2.
#[tokio::test]
async fn dist_http_forwarded_query_defaults_to_normalized_representation() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:fwd-normalized");
    let reg_id = topology.id("registration:fwd-normalized");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .query_entities("type=Vehicle&options=keyValues", "application/ld+json", Some(context_link()))
            .await
            .expect("forwarded query"),
        200,
    );
    let forwarded = mock.next_request(Duration::from_secs(5)).await;
    assert!(
        forwarded.uri.query().is_none_or(|query| !query.contains("keyValues")),
        "a forwarded query must default to normalized, got {:?}",
        forwarded.uri.query()
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A single source that cannot serve the requested representation maps to
/// `501 Not Implemented` (clause 6.3.16). Confirms REQ-HTTP-3.
#[tokio::test]
async fn dist_http_forwarded_query_unsupported_representation_returns_501() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:fwd-501");
    let reg_id = topology.id("registration:fwd-501");
    let mock = ContextSourceAdapter::start().await;

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, "exclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub(Method::GET, FORWARDED_ENTITIES_PATH, 501).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=Vehicle", "application/ld+json", Some(context_link()))
        .await
        .expect("forwarded query");
    assert_status(&response, 501);

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `GeoJSON` `Accept` on a query returns a `FeatureCollection` (clause 6.3.7).
/// Confirms REQ-HTTP-56.
#[tokio::test]
async fn dist_http_query_geojson_accept_returns_feature_collection() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:geo-collection");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&located_vehicle(&entity_id))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_entities("type=Vehicle", "application/geo+json", Some(context_link()))
        .await
        .expect("geojson query");
    assert_status(&response, 200);
    assert_eq!(
        json_body(&response)["type"].as_str(),
        Some("FeatureCollection"),
        "a GeoJSON query returns a FeatureCollection"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A `GeoJSON` `Accept` on a retrieve returns a `Feature` (clause 6.3.7).
/// Confirms REQ-HTTP-67.
#[tokio::test]
async fn dist_http_retrieve_geojson_accept_returns_feature() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:geo-feature");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&located_vehicle(&entity_id))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .get_entity(&entity_id, "", "application/geo+json", Some(context_link()))
        .await
        .expect("geojson retrieve");
    assert_status(&response, 200);
    assert_eq!(json_body(&response)["type"].as_str(), Some("Feature"), "a GeoJSON retrieve returns a Feature");

    topology.finish().await.expect("topology cleanup");
}

/// A query carrying `Prefer: ngsi-ld=1.8` is amended to that version and answers
/// `203` with `Preference-Applied` (Table 6.4.3.2-3: "203 Non-Authoritative
/// Information ... returning an altered response body, amended to conform to a
/// specific version of the NGSI-LD specification as mandated in clause 4.3.6.8.
/// The response shall also include a "Preference-Applied" HTTP header set to
/// "ngsi-ld=<conformant-version>""). Without the `Prefer` header there is
/// nothing to amend, so the header is what turns a `200` into a `203`.
/// Confirms REQ-HTTP-55.
#[tokio::test]
async fn dist_http_query_version_amended_returns_203_preference_applied() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:amend-query");
    let reg_id = topology.id("registration:amend-query");
    let mock = ContextSourceAdapter::start().await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, "inclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, FORWARDED_ENTITIES_PATH, 200, &json!([])).await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .header("prefer", PREFER_PREVIOUS_VERSION)
        .send()
        .await
        .expect("amended query");
    assert_status(&response, 203);
    assert!(
        response.headers.get("preference-applied").is_some(),
        "a version-amended response carries Preference-Applied"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A retrieve carrying `Prefer: ngsi-ld=1.8` is amended to that version and
/// answers `203` with `Preference-Applied` (clause 5.7.1.4: "If the Prefer
/// Header [26] is set to "ngsi-ld=<version>" then the `ContextBroker` shall
/// endeavour to amend the JSON-LD object to conform to the specified version of
/// the NGSI-LD specification as mandated in clause 4.3.6.8, and return a
/// Preference-Applied Header set to "ngsi-ld=<conformant-version>" in the
/// response"). Confirms REQ-HTTP-66.
#[tokio::test]
async fn dist_http_retrieve_version_amended_returns_203_preference_applied() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:amend-retrieve");
    let reg_id = topology.id("registration:amend-retrieve");
    let mock = ContextSourceAdapter::start().await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&retrieval_source(&reg_id, &mock.url(""), &entity_id, "exclusive"))
            .await
            .expect("register source"),
        201,
    );
    mock.set_stub_with_body(Method::GET, &forwarded_entity_path(&entity_id), 200, &vehicle(&entity_id, 5))
        .await;

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, &format!("/ngsi-ld/v1/entities/{}", urlencoding::encode(&entity_id)))
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .header("prefer", PREFER_PREVIOUS_VERSION)
        .send()
        .await
        .expect("amended retrieve");
    assert_status(&response, 203);
    assert!(
        response.headers.get("preference-applied").is_some(),
        "a version-amended response carries Preference-Applied"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An `entityMaps` request carrying `Prefer: ngsi-ld=1.8` is amended to that
/// version and answers `203` with `Preference-Applied` (clause 6.3.6: "If the
/// Prefer Header [26] is set to "ngsi-ld=<version>" then implementations shall
/// set Preference-Applied header to "ngsi-ld=<conformant-version>" in the
/// returned response"). Confirms REQ-HTTP-99.
#[tokio::test]
async fn dist_http_entity_maps_version_amended_returns_203_preference_applied() {
    let topology = topology::single().await.expect("single-broker topology");
    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entityMaps?type=Vehicle")
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .header("prefer", PREFER_PREVIOUS_VERSION)
        .send()
        .await
        .expect("entity-maps request");
    assert_status(&response, 203);
    assert!(
        response.headers.get("preference-applied").is_some(),
        "a version-amended response carries Preference-Applied"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A profile `Link` header naming an earlier NGSI-LD version is accepted
/// alongside the context `Link`, and the request still succeeds (clause 6.3.21:
/// "if an HTTP Link header with a profile parameter in accordance with IETF RFC
/// 6906 [33], is found set to <https://uri.etsi.org/ngsi-ld/profile/version>
/// then implementations that are only partially capable of interpreting the
/// datatypes conformant to the supplied NGSI-LD version may use this
/// information to allow the payload to be accepted within the constraints of
/// the current implementation (see clause 5.5.4) through amending the payload
/// body and applying the fallbacks defined within clause 4.3.6.8").
/// Confirms REQ-HTTP-45.
#[tokio::test]
async fn dist_http_profile_link_header_version_accepted_with_fallbacks() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:profile");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("local create"),
        201,
    );

    let response = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::GET, "/ngsi-ld/v1/entities?type=Vehicle")
        .header("accept", "application/ld+json")
        .header("link", context_link_header())
        .header("link", PROFILE_LINK_PREVIOUS_VERSION)
        .send()
        .await
        .expect("profile-link query");
    assert_status(&response, 200);

    topology.finish().await.expect("topology cleanup");
}
