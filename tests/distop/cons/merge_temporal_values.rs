//! Retrieve Temporal Entity forwarding in the simplified temporal representation
//! (clauses 4.5.9 and 5.7.3.4): a source is asked for the normalized instances of
//! the window whatever representation the client asked for, so the instances two
//! holders keep of one `datasetId` are instances of one dataset, and the
//! `temporalValues` object of that dataset carries the pairs of both. Before this,
//! each holder answered its own `temporalValues` object and the merge appended a
//! second object for the same `datasetId` instead of merging it.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::entity,
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;

/// The whole-history temporal query window: everything before 2030.
const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// Budget for a forwarded request to reach the mock context source.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);

/// The path a broker appends when forwarding a temporal retrieval.
const FORWARDED_TEMPORAL: &str = "/ngsi-ld/v1/temporal/entities/";

/// A temporal `Property` instance of the dataset `dataset_id`.
fn observed_dataset(value: i64, observed_at: &str, dataset_id: &str) -> Value {
    json!({ "type": "Property", "value": value, "observedAt": observed_at, "datasetId": dataset_id })
}

/// The objects one attribute of a `temporalValues` answer is written as: a
/// single object for one dataset, an array of them for several.
fn dataset_objects(attribute: &Value) -> Vec<&Value> {
    match attribute {
        Value::Array(objects) => objects.iter().collect(),
        object @ Value::Object(_) => vec![object],
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Vec::new(),
    }
}

/// The `[value, instant]` pairs of every object, in answer order.
fn pairs(objects: &[&Value]) -> Vec<(i64, String)> {
    objects
        .iter()
        .flat_map(|object| object["values"].as_array().into_iter().flatten())
        .map(|pair| {
            (
                pair[0].as_i64().expect("a pair starts with its value"),
                pair[1].as_str().expect("a pair ends with its instant").to_owned(),
            )
        })
        .collect()
}

/// Two brokers each hold instances of one `speed` dataset; the front broker
/// answers `selection` over both.
async fn merged_speed(label: &str, selection: &str) -> Value {
    let topology = topology::pair().await.expect("two-broker topology");
    let entity_id = topology.id(&format!("Vehicle:{label}"));
    let dataset_id = topology.id("dataset:a");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let holder = topology.broker(BrokerRole::B).expect("topology broker B");

    assert_status(
        &front
            .client()
            .create_temporal_entity(&entity(
                &entity_id,
                "Vehicle",
                &[(
                    "speed",
                    json!([
                        observed_dataset(10, "2020-08-01T12:00:00Z", &dataset_id),
                        observed_dataset(20, "2020-08-01T12:05:00Z", &dataset_id)
                    ]),
                )],
            ))
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
                &[(
                    "speed",
                    json!([
                        observed_dataset(30, "2020-08-01T12:10:00Z", &dataset_id),
                        observed_dataset(40, "2020-08-01T12:15:00Z", &dataset_id)
                    ]),
                )],
            ))
            .await
            .expect("holder"),
        201,
    );
    let registration = CsrBuilder::new(&topology.id(&format!("registration:{label}")), holder.peer_url())
        .tenant(topology.tenant())
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let retrieved = front
        .client()
        .get_temporal_entity(&entity_id, &format!("{BEFORE_2030}&{selection}"), None)
        .await
        .expect("retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    let dataset_id = dataset_id.as_str();
    assert_eq!(
        dataset_objects(&body["speed"])
            .iter()
            .filter(|object| object["datasetId"].as_str() == Some(dataset_id))
            .count(),
        1,
        "the dataset is one object of the answer, not one per holder: {body}"
    );

    topology.finish().await.expect("topology cleanup");
    body
}

/// What the merged `speed` of `body` must hold: the pairs of both holders, once
/// each, in one object.
fn assert_pairs_of_both_holders(body: &Value) {
    let objects = dataset_objects(&body["speed"]);
    assert_eq!(objects.len(), 1, "one dataset, one object: {body}");
    let mut merged = pairs(&objects);
    merged.sort();
    assert_eq!(
        merged,
        vec![
            (10, "2020-08-01T12:00:00Z".to_owned()),
            (20, "2020-08-01T12:05:00Z".to_owned()),
            (30, "2020-08-01T12:10:00Z".to_owned()),
            (40, "2020-08-01T12:15:00Z".to_owned())
        ],
        "the object carries the pairs of both holders: {body}"
    );
}

/// Two holders of one `datasetId` answer a `format=temporalValues` retrieval
/// with one object whose pairs are the union of their instances (clauses 4.5.5,
/// 4.5.9). Confirms T-D2.
#[tokio::test]
async fn distop_merge_temporal_values_format_merges_the_pairs_of_one_dataset() {
    assert_pairs_of_both_holders(&merged_speed("merge-values-format", "format=temporalValues").await);
}

/// The same holds for the `options=temporalValues` spelling of the representation.
#[tokio::test]
async fn distop_merge_temporal_values_options_merges_the_pairs_of_one_dataset() {
    assert_pairs_of_both_holders(&merged_speed("merge-values-options", "options=temporalValues").await);
}

/// A source is asked for the normalized instances of the window whatever
/// representation the client asked for: the request it receives names no
/// `temporalValues`, in `format` or in `options`.
#[tokio::test]
async fn distop_merge_temporal_values_a_source_is_asked_for_normalized_instances() {
    let topology = topology::single().await.expect("single-broker topology");
    let front = topology.broker(BrokerRole::A).expect("topology broker A");
    let mock = ContextSourceAdapter::start().await;
    let entity_id = topology.id("Vehicle:merge-values-forwarded");
    let registration = CsrBuilder::new(&topology.id("registration:merge-values-forwarded"), &mock.url(""))
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["speed"])
        .operations(&["retrieveTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);
    let answer = entity(
        &entity_id,
        "Vehicle",
        &[("speed", json!([observed_dataset(10, "2020-08-01T12:00:00Z", "urn:ngsi-ld:Dataset:a")]))],
    );

    for selection in ["format=temporalValues", "options=temporalValues"] {
        mock.set_stub_with_body(Method::GET, &format!("{FORWARDED_TEMPORAL}{entity_id}"), 200, &answer)
            .await;
        let retrieved = front
            .client()
            .get_temporal_entity(&entity_id, &format!("{BEFORE_2030}&{selection}"), None)
            .await
            .expect("retrieve");
        assert_status(&retrieved, 200);
        let forwarded = mock.next_request(CAPTURE_TIMEOUT).await;
        let query = forwarded.uri.query().unwrap_or_default();
        assert!(!query.contains("temporalValues"), "{selection}: the source is asked for instances: {query}");
    }

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
