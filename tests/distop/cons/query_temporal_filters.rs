//! Query Temporal post-aggregation `datasetId` filtering, distributed pagination,
//! csf forwarding, and local `orderBy id` (clauses 5.7.4.4/.5, 5.5.9).
//! Forwarding-selection and merge rows live in `crate::cons::query_temporal`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::entity,
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use serde_json::{Value, json};
use std::time::Duration;
use urlencoding::encode;

/// A single temporal `Property` instance at the given `observedAt`.
fn observed(value: i64, observed_at: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at })
}

/// A temporal `Property` instance carrying a `datasetId`.
fn observed_dataset(value: i64, observed_at: &str, dataset_id: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at, "datasetId": dataset_id })
}

/// The `id` strings of a temporal array body.
fn ids(body: &Value) -> Vec<String> {
    body.as_array()
        .map(|entries| entries.iter().filter_map(|entry| entry["id"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// The instances of one attribute on the temporal entity with the given id.
fn attribute_instances(body: &Value, id: &str, attribute: &str) -> Vec<Value> {
    let entity = body
        .as_array()
        .and_then(|entries| entries.iter().find(|entry| entry["id"].as_str() == Some(id)))
        .cloned()
        .unwrap_or(Value::Null);
    entity[attribute].as_array().cloned().unwrap_or_default()
}

const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// `datasetId` filters S7 per attribute into S8: a specific id keeps matching
/// instances, `@none` the default, and a datasetId no instance carries removes the
/// attribute (clause 5.7.4.4). Confirms REQ-CONS-67.
#[tokio::test]
async fn distop_query_temporal_datasetid_filters_s7_empty_removed() {
    let topology = topology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons67");
    let dataset_id = topology.id("dataset:2");
    let front = topology.broker(BrokerRole::A);
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
    let registration_id = topology.id("registration:cons67");
    let registration = CsrBuilder::new(&registration_id, holder.peer_url())
        .mode("inclusive")
        .operations(&["queryTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let by_dataset = front
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&datasetId={}", encode(&dataset_id)), None)
        .await
        .expect("dataset");
    assert_status(&by_dataset, 200);
    let dataset_instances = attribute_instances(&json_body(&by_dataset), &entity_id, "speed");
    assert!(
        !dataset_instances.is_empty()
            && dataset_instances
                .iter()
                .all(|instance| instance["datasetId"].as_str() == Some(dataset_id.as_str())),
        "only the requested-datasetId instances are kept"
    );

    let by_none = front
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&datasetId=@none"), None)
        .await
        .expect("none");
    assert_status(&by_none, 200);
    assert!(
        attribute_instances(&json_body(&by_none), &entity_id, "speed")
            .iter()
            .all(|instance| instance.get("datasetId").is_none()),
        "@none keeps only default instances"
    );

    let absent = topology.id("dataset:absent");
    let by_absent = front
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&datasetId={}", encode(&absent)), None)
        .await
        .expect("absent");
    assert_status(&by_absent, 200);
    assert!(
        attribute_instances(&json_body(&by_absent), &entity_id, "speed").is_empty(),
        "an attribute emptied by filtering is removed"
    );

    topology.finish().await.expect("topology cleanup");
}

/// Without a `datasetId` parameter S8 equals S7: the aggregated result passes
/// through unchanged, retaining every `datasetId` (clause 5.7.4.4). Confirms
/// REQ-CONS-68.
#[tokio::test]
async fn distop_query_temporal_no_datasetid_s8_equals_s7() {
    let topology = topology::chain().await.expect("three-broker topology");
    let entity_id = topology.id("Vehicle:cons68");
    let dataset_id = topology.id("dataset:2");
    let front = topology.broker(BrokerRole::A);
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
    let registration_id = topology.id("registration:cons68");
    let registration = CsrBuilder::new(&registration_id, holder.peer_url())
        .mode("inclusive")
        .operations(&["queryTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let response = front
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}"), None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let instances = attribute_instances(&json_body(&response), &entity_id, "speed");
    assert!(
        instances.iter().any(|instance| instance.get("datasetId").is_none()),
        "the default instance is retained without a datasetId filter"
    );
    assert!(
        instances.iter().any(|instance| instance["datasetId"].as_str() == Some(dataset_id.as_str())),
        "the datasetId instance is retained without a datasetId filter"
    );

    topology.finish().await.expect("topology cleanup");
}

/// Distributed temporal pagination covers every candidate across pages with no
/// duplicate and no page exceeding the limit (clause 5.5.9). Confirms REQ-CONS-71.
#[tokio::test]
async fn distop_query_temporal_pagination_over_candidates() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A);
    let mock = ContextSourceAdapter::start().await;

    let candidates: Vec<String> = (0..5).map(|index| topology.id(&format!("Vehicle:cons71-{index}"))).collect();
    let array = Value::Array(
        candidates
            .iter()
            .map(|id| entity(id, "Vehicle", &[("speed", json!([observed(10, "2020-08-01T12:00:00Z")]))]))
            .collect(),
    );
    let registration_id = topology.id("registration:cons71");
    let registration = CsrBuilder::new(&registration_id, &mock.url(""))
        .mode("inclusive")
        .operations(&["queryTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &array).await;

    let mut seen: Vec<String> = Vec::new();
    for offset in [0, 2, 4] {
        let page = front
            .client()
            .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&limit=2&offset={offset}&count=true"), None)
            .await
            .expect("page");
        assert_status(&page, 200);
        let page_ids = ids(&json_body(&page));
        assert!(page_ids.len() <= 2, "no page exceeds the limit, got {page_ids:?}");
        for id in page_ids {
            assert!(!seen.contains(&id), "no id appears on two pages: {id}");
            seen.push(id);
        }
    }
    for candidate in &candidates {
        assert!(seen.contains(candidate), "every candidate is covered across pages, missing {candidate}");
    }

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `csf` expression is forwarded byte-identical into the downstream discovery a
/// chained source performs for a temporal query (clause 5.7.4.4). Confirms
/// REQ-CONS-72.
#[tokio::test]
async fn distop_query_temporal_csf_forwarded_unchanged() {
    let topology = topology::chain().await.expect("three-broker topology");
    let front = topology.broker(BrokerRole::A);
    let middle = topology.broker(BrokerRole::B);
    let mock = ContextSourceAdapter::start().await;

    let downstream_id = topology.id("registration:cons72-downstream");
    let downstream = CsrBuilder::new(&downstream_id, &mock.url(""))
        .mode("inclusive")
        .operations(&["queryTemporal"])
        .build();
    assert_status(&middle.client().create_registration(&downstream).await.expect("downstream register"), 201);
    mock.set_stub_with_body(Method::GET, "/ngsi-ld/v1/temporal/entities", 200, &json!([])).await;

    let hop_id = topology.id("registration:cons72-hop");
    let hop = CsrBuilder::new(&hop_id, middle.peer_url())
        .mode("inclusive")
        .operations(&["queryTemporal"])
        .build();
    assert_status(&front.client().create_registration(&hop).await.expect("hop register"), 201);

    let csf = "mode==inclusive";
    assert_status(
        &front
            .client()
            .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&csf={}", encode(csf)), None)
            .await
            .expect("query"),
        200,
    );

    let forwarded = mock.next_request(Duration::from_secs(3)).await;
    let query = forwarded.uri.query().unwrap_or_default();
    assert!(
        query.contains("csf"),
        "the csf expression is forwarded into downstream temporal discovery, got {query:?}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `local=true` temporal query with `orderBy=id` orders the result by entity id
/// (clause 5.7.4.5). Confirms REQ-CONS-73.
#[tokio::test]
async fn distop_query_temporal_orderby_id_local_orders_result() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A);
    let first = topology.id("Vehicle:cons73-a");
    let second = topology.id("Vehicle:cons73-b");
    let third = topology.id("Vehicle:cons73-c");
    for id in [&third, &first, &second] {
        assert_status(
            &front
                .client()
                .create_temporal_entity(&entity(id, "Vehicle", &[("speed", json!([observed(10, "2020-08-01T12:00:00Z")]))]))
                .await
                .expect("create"),
            201,
        );
    }

    let response = front
        .client()
        .query_temporal_entities(&format!("type=Vehicle&{BEFORE_2030}&local=true&orderBy=id"), None)
        .await
        .expect("query");
    assert_status(&response, 200);
    let ordered: Vec<String> = ids(&json_body(&response))
        .into_iter()
        .filter(|id| [&first, &second, &third].contains(&id))
        .collect();
    assert_eq!(ordered, vec![first, second, third], "temporal results are ordered by id");

    topology.finish().await.expect("topology cleanup");
}
