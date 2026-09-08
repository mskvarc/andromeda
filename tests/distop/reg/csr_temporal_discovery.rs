//! Latest-vs-temporal purpose split and interval overlap matching in
//! registration discovery (clause 5.9.1, matched per clause 5.10.2.4). A
//! registration's `managementInterval` is matched by `createdAt`/`modifiedAt`-
//! scoped queries and its `observationInterval` by `observedAt`-scoped queries;
//! a `between` window matches when the interval overlaps it. Every discovery
//! query names `type=Vehicle`: clause 5.10.2.4 requires at least one of a type
//! selector, attribute list, query or geoquery, and rejects a query with only
//! entity identifiers as too wide. All `T-LOCAL`.

use crate::{
    oracle::{assert_status, json_body},
    reg::PEER_ENDPOINT,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use serde_json::{Value, json};
use urlencoding::encode;

/// The registration ids present in a discovery response array.
fn registration_ids(body: &Value) -> Vec<String> {
    body.as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
        .collect()
}

/// A registration-discovery query for entity `entity_id` over a `between` window,
/// optionally scoped to a specific time property.
fn temporal_query(entity_id: &str, from: &str, to: &str, timeproperty: Option<&str>) -> String {
    let mut query = format!(
        "type=Vehicle&id={}&timerel=between&timeAt={}&endTimeAt={}",
        encode(entity_id),
        encode(from),
        encode(to)
    );
    if let Some(timeproperty) = timeproperty {
        query.push_str("&timeproperty=");
        query.push_str(timeproperty);
    }
    query
}

/// A `managementInterval` matches a `modifiedAt`-scoped temporal discovery whose
/// window overlaps it, and not one that is disjoint (clause 5.9.1). Confirms
/// REQ-REG-18.
#[tokio::test]
async fn reg_managementinterval_overlap_matched_on_temporal_query() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:mgmt-interval");
    let registration_id = topology.id("registration:mgmt-interval");
    let mut body = CsrBuilder::new(&registration_id, PEER_ENDPOINT).entity_id(&entity_id).build();
    body["managementInterval"] = json!({ "startAt": "2020-01-01T00:00:00Z", "endAt": "2020-12-31T00:00:00Z" });
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&body)
            .await
            .expect("create registration"),
        201,
    );

    let overlapping = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_registrations(
            &temporal_query(&entity_id, "2020-06-01T00:00:00Z", "2020-06-30T00:00:00Z", Some("modifiedAt")),
            None,
        )
        .await
        .expect("overlapping temporal discovery");
    assert_status(&overlapping, 200);
    assert!(
        registration_ids(&json_body(&overlapping)).contains(&registration_id),
        "an overlapping window returns the registration"
    );

    let disjoint = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_registrations(
            &temporal_query(&entity_id, "2025-01-01T00:00:00Z", "2025-12-31T00:00:00Z", Some("modifiedAt")),
            None,
        )
        .await
        .expect("disjoint temporal discovery");
    assert_status(&disjoint, 200);
    assert!(
        !registration_ids(&json_body(&disjoint)).contains(&registration_id),
        "a disjoint window does not return the registration"
    );

    topology.finish().await.expect("topology cleanup");
}

/// An `observationInterval` matches an `observedAt`-scoped temporal discovery
/// whose window overlaps it, and not one that is disjoint (clause 5.9.1).
/// Confirms REQ-REG-21.
#[tokio::test]
async fn reg_observationinterval_overlap_matched_on_observedat_query() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:obs-interval");
    let registration_id = topology.id("registration:obs-interval");
    let mut body = CsrBuilder::new(&registration_id, PEER_ENDPOINT).entity_id(&entity_id).build();
    body["observationInterval"] = json!({ "startAt": "2020-01-01T00:00:00Z", "endAt": "2020-12-31T00:00:00Z" });
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&body)
            .await
            .expect("create registration"),
        201,
    );

    let overlapping = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_registrations(
            &temporal_query(&entity_id, "2020-06-01T00:00:00Z", "2020-06-30T00:00:00Z", Some("observedAt")),
            None,
        )
        .await
        .expect("overlapping observedAt discovery");
    assert_status(&overlapping, 200);
    assert!(
        registration_ids(&json_body(&overlapping)).contains(&registration_id),
        "an overlapping window returns the registration"
    );

    let disjoint = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_registrations(
            &temporal_query(&entity_id, "2025-01-01T00:00:00Z", "2025-12-31T00:00:00Z", Some("observedAt")),
            None,
        )
        .await
        .expect("disjoint observedAt discovery");
    assert_status(&disjoint, 200);
    assert!(
        !registration_ids(&json_body(&disjoint)).contains(&registration_id),
        "a disjoint window does not return the registration"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A non-temporal discovery returns latest-info registrations and excludes ones
/// declaring only a temporal interval (clause 5.9.1). Confirms REQ-REG-50.
#[tokio::test]
async fn reg_no_temporal_query_returns_only_latest_csrs() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:latest-vs-temporal");
    let latest_id = topology.id("registration:latest-only");
    let temporal_id = topology.id("registration:temporal-only");

    let latest = CsrBuilder::new(&latest_id, PEER_ENDPOINT).entity_id(&entity_id).build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&latest)
            .await
            .expect("create latest CSR"),
        201,
    );
    let mut temporal = CsrBuilder::new(&temporal_id, PEER_ENDPOINT).entity_id(&entity_id).build();
    temporal["managementInterval"] = json!({ "startAt": "2020-01-01T00:00:00Z", "endAt": "2020-12-31T00:00:00Z" });
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&temporal)
            .await
            .expect("create temporal CSR"),
        201,
    );

    let result = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_registrations(&format!("type=Vehicle&id={}", encode(&entity_id)), None)
        .await
        .expect("non-temporal discovery");
    assert_status(&result, 200);
    let ids = registration_ids(&json_body(&result));
    assert!(ids.contains(&latest_id), "a non-temporal discovery includes the latest-info registration");
    assert!(!ids.contains(&temporal_id), "a non-temporal discovery excludes the temporal-only registration");

    topology.finish().await.expect("topology cleanup");
}

/// A temporal discovery returns overlapping-interval registrations and excludes
/// latest-only ones (clause 5.9.1). Confirms REQ-REG-51.
#[tokio::test]
async fn reg_temporal_query_returns_only_interval_csrs() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:temporal-vs-latest");
    let latest_id = topology.id("registration:temporal-latest");
    let temporal_id = topology.id("registration:temporal-interval");

    let latest = CsrBuilder::new(&latest_id, PEER_ENDPOINT).entity_id(&entity_id).build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&latest)
            .await
            .expect("create latest CSR"),
        201,
    );
    let mut temporal = CsrBuilder::new(&temporal_id, PEER_ENDPOINT).entity_id(&entity_id).build();
    temporal["managementInterval"] = json!({ "startAt": "2020-01-01T00:00:00Z", "endAt": "2020-12-31T00:00:00Z" });
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&temporal)
            .await
            .expect("create temporal CSR"),
        201,
    );

    let result = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_registrations(
            &temporal_query(&entity_id, "2020-06-01T00:00:00Z", "2020-06-30T00:00:00Z", Some("modifiedAt")),
            None,
        )
        .await
        .expect("temporal discovery");
    assert_status(&result, 200);
    let ids = registration_ids(&json_body(&result));
    assert!(
        ids.contains(&temporal_id),
        "a temporal discovery includes the overlapping-interval registration"
    );
    assert!(!ids.contains(&latest_id), "a temporal discovery excludes the latest-only registration");

    topology.finish().await.expect("topology cleanup");
}

/// A registration carrying both latest info and a `managementInterval` is treated
/// as a single purpose, appearing in exactly one of the non-temporal / temporal
/// discovery modes (clause 5.9.1). Confirms REQ-REG-49.
#[tokio::test]
async fn reg_csr_combining_latest_and_temporal_single_purpose() {
    let topology = topology::single().await.expect("single-broker topology");
    let entity_id = topology.id("Vehicle:combined-purpose");
    let registration_id = topology.id("registration:combined-purpose");
    let mut body = CsrBuilder::new(&registration_id, PEER_ENDPOINT).entity_id(&entity_id).build();
    body["managementInterval"] = json!({ "startAt": "2020-01-01T00:00:00Z", "endAt": "2020-12-31T00:00:00Z" });
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&body)
            .await
            .expect("create combined CSR"),
        201,
    );

    let non_temporal = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_registrations(&format!("type=Vehicle&id={}", encode(&entity_id)), None)
        .await
        .expect("non-temporal discovery");
    assert_status(&non_temporal, 200);
    let temporal = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .query_registrations(
            &temporal_query(&entity_id, "2020-06-01T00:00:00Z", "2020-06-30T00:00:00Z", Some("modifiedAt")),
            None,
        )
        .await
        .expect("temporal discovery");
    assert_status(&temporal, 200);

    let in_latest = registration_ids(&json_body(&non_temporal)).contains(&registration_id);
    let in_temporal = registration_ids(&json_body(&temporal)).contains(&registration_id);
    assert!(
        in_latest ^ in_temporal,
        "a CSR carrying both latest info and an interval is a single purpose, appearing in exactly one discovery mode (latest={in_latest}, temporal={in_temporal})"
    );

    topology.finish().await.expect("topology cleanup");
}
