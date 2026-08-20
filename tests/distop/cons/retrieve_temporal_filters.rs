//! Retrieve Temporal auxiliary supplementation, post-merge `datasetId` filtering,
//! and `lastN` projection (clause 5.7.3.4). Forwarding-selection and base-series
//! rows live in `crate::cons::retrieve_temporal`.

use crate::{
    entity::entity,
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{BrokerRole, DistributedTopology},
};
use serde_json::{Value, json};
use urlencoding::encode;

/// A single temporal `Property` instance at the given `observedAt`.
fn observed(value: i64, observed_at: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at })
}

/// A temporal `Property` instance carrying a `datasetId`.
fn observed_dataset(value: i64, observed_at: &str, dataset_id: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at, "datasetId": dataset_id })
}

/// The `observedAt` values of one attribute's instances in a temporal body.
fn observed_ats(body: &Value, attribute: &str) -> Vec<String> {
    body[attribute]
        .as_array()
        .map(|instances| {
            instances
                .iter()
                .filter_map(|instance| instance["observedAt"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// An auxiliary source adds a temporal instance only where that timeproperty value
/// is absent: a duplicate `observedAt` is suppressed while a new instance and a new
/// attribute are added (clause 5.7.3.4). Confirms REQ-CONS-47.
#[tokio::test]
async fn distop_retrieve_temporal_auxiliary_added_only_if_timeproperty_absent() {
    let topology = DistributedTopology::pair().await.expect("two-broker topology");
    let entity_id = topology.id("Vehicle:cons47");
    let front = topology.front();
    let peer = topology.broker(BrokerRole::B);

    assert_status(
        &front
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("speed", json!([observed(50, "2020-08-01T12:00:00Z")]))]))
            .await
            .expect("local"),
        201,
    );
    let auxiliary = entity(
        &entity_id,
        "Vehicle",
        &[
            ("speed", json!([observed(99, "2020-08-01T12:00:00Z"), observed(70, "2020-08-01T12:30:00Z")])),
            ("fuelLevel", json!([observed(40, "2020-08-01T12:30:00Z")])),
        ],
    );
    assert_status(&peer.client().create_temporal_entity(&auxiliary).await.expect("peer"), 201);

    let registration_id = topology.id("registration:cons47");
    let registration = CsrBuilder::new(&registration_id, peer.peer_url())
        .mode("auxiliary")
        .entity_id(&entity_id)
        .property_names(&["speed", "fuelLevel"])
        .operations(&["retrieveTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let retrieved = front.client().get_temporal_entity(&entity_id, BEFORE_2030, None).await.expect("retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    let at_first: Vec<&Value> = body["speed"]
        .as_array()
        .map(|instances| {
            instances
                .iter()
                .filter(|instance| instance["observedAt"].as_str() == Some("2020-08-01T12:00:00Z"))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(at_first.len(), 1, "the duplicate timeproperty instance is suppressed: {body}");
    assert_eq!(
        at_first[0]["value"].as_i64(),
        Some(50),
        "the present local instance is kept over the auxiliary duplicate"
    );
    assert!(
        observed_ats(&body, "speed").contains(&"2020-08-01T12:30:00Z".to_owned()),
        "the new auxiliary instance is added: {body}"
    );
    assert!(body.get("fuelLevel").is_some(), "the absent auxiliary attribute is added: {body}");

    topology.finish().await.expect("topology cleanup");
}

/// A `datasetId` query parameter keeps only matching instances across the temporal
/// history; `@none` keeps the default (clause 5.7.3.4). Confirms REQ-CONS-49.
#[tokio::test]
async fn distop_retrieve_temporal_datasetid_filters_s1() {
    let topology = DistributedTopology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons49");
    let dataset_id = topology.id("dataset:2");
    let front = topology.front();
    let holder = topology.broker(BrokerRole::C);

    assert_status(
        &front
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("speed", json!([observed(10, "2020-08-01T12:00:00Z")]))]))
            .await
            .expect("local"),
        201,
    );
    assert_status(
        &holder
            .client()
            .create_temporal_entity(&entity(
                &entity_id,
                "Vehicle",
                &[("speed", json!([observed_dataset(30, "2020-08-01T12:05:00Z", &dataset_id)]))],
            ))
            .await
            .expect("holder"),
        201,
    );
    let registration_id = topology.id("registration:cons49");
    let registration = CsrBuilder::new(&registration_id, holder.peer_url())
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let by_dataset = front
        .client()
        .get_temporal_entity(&entity_id, &format!("{BEFORE_2030}&datasetId={}", encode(&dataset_id)), None)
        .await
        .expect("dataset");
    assert_status(&by_dataset, 200);
    let dataset_body = json_body(&by_dataset);
    let dataset_instances = dataset_body["speed"].as_array().cloned().unwrap_or_default();
    assert!(
        !dataset_instances.is_empty()
            && dataset_instances
                .iter()
                .all(|instance| instance["datasetId"].as_str() == Some(dataset_id.as_str())),
        "only the requested-datasetId instances are retained: {dataset_body}"
    );

    let by_none = front
        .client()
        .get_temporal_entity(&entity_id, &format!("{BEFORE_2030}&datasetId=@none"), None)
        .await
        .expect("none");
    assert_status(&by_none, 200);
    let none_body = json_body(&by_none);
    assert!(
        none_body["speed"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .all(|instance| instance.get("datasetId").is_none()),
        "@none retains only default instances: {none_body}"
    );

    topology.finish().await.expect("topology cleanup");
}

/// A `datasetId` no instance carries removes the whole attribute from S1 while
/// other attributes are retained (clause 5.7.3.4). Confirms REQ-CONS-50.
#[tokio::test]
async fn distop_retrieve_temporal_datasetid_no_match_removes_attribute() {
    let topology = DistributedTopology::pair().await.expect("two-broker topology");
    let entity_id = topology.id("Vehicle:cons50");
    let dataset_id = topology.id("dataset:2");
    let front = topology.front();

    let local = entity(
        &entity_id,
        "Vehicle",
        &[
            ("speed", json!([observed(10, "2020-08-01T12:00:00Z")])),
            ("fuelLevel", json!([observed_dataset(40, "2020-08-01T12:00:00Z", &dataset_id)])),
        ],
    );
    assert_status(&front.client().create_temporal_entity(&local).await.expect("local"), 201);

    let absent = topology.id("dataset:absent");
    let retrieved = front
        .client()
        .get_temporal_entity(&entity_id, &format!("{BEFORE_2030}&datasetId={}", encode(&absent)), None)
        .await
        .expect("retrieve");
    assert_status(&retrieved, 200);
    assert!(
        json_body(&retrieved)["speed"].as_array().cloned().unwrap_or_default().is_empty(),
        "speed carries no matching datasetId instance and is removed"
    );

    topology.finish().await.expect("topology cleanup");
}

/// `lastN` limits the projected instances to the most recent across the merged
/// history, applied post-merge rather than per source (clause 5.7.3.4). Confirms
/// REQ-CONS-51.
#[tokio::test]
async fn distop_retrieve_temporal_lastn_limits_projected_instances() {
    let topology = DistributedTopology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons51");
    let front = topology.front();
    let holder_b = topology.broker(BrokerRole::B);
    let holder_c = topology.broker(BrokerRole::C);

    assert_status(
        &front
            .client()
            .create_temporal_entity(&entity(
                &entity_id,
                "Vehicle",
                &[("speed", json!([observed(10, "2020-08-01T12:00:00Z"), observed(20, "2020-08-01T12:01:00Z")]))],
            ))
            .await
            .expect("local"),
        201,
    );
    assert_status(
        &holder_b
            .client()
            .create_temporal_entity(&entity(
                &entity_id,
                "Vehicle",
                &[("speed", json!([observed(30, "2020-08-01T12:02:00Z"), observed(40, "2020-08-01T12:03:00Z")]))],
            ))
            .await
            .expect("b"),
        201,
    );
    assert_status(
        &holder_c
            .client()
            .create_temporal_entity(&entity(&entity_id, "Vehicle", &[("speed", json!([observed(50, "2020-08-01T12:04:00Z")]))]))
            .await
            .expect("c"),
        201,
    );

    for role in [BrokerRole::B, BrokerRole::C] {
        let peer = topology.broker(role);
        let registration_id = topology.id(&format!("registration:cons51-{role:?}"));
        let registration = CsrBuilder::new(&registration_id, peer.peer_url())
            .mode("inclusive")
            .entity_id(&entity_id)
            .property_names(&["speed"])
            .operations(&["retrieveTemporal"])
            .build();
        assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    }

    let retrieved = front
        .client()
        .get_temporal_entity(&entity_id, &format!("{BEFORE_2030}&attrs=speed&lastN=2"), None)
        .await
        .expect("retrieve");
    assert_status(&retrieved, 200);
    let mut ats = observed_ats(&json_body(&retrieved), "speed");
    ats.sort();
    assert_eq!(
        ats,
        vec!["2020-08-01T12:03:00Z".to_owned(), "2020-08-01T12:04:00Z".to_owned()],
        "exactly the two most-recent instances across the merged history are projected"
    );

    topology.finish().await.expect("topology cleanup");
}
