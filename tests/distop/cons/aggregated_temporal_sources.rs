//! `aggregatedValues` over several holders of one temporal entity (clauses
//! 4.5.19 and 5.7.3.4). A source is asked for its aggregates as the client asked
//! for them, and an aggregate answered for an attribute and `datasetId` the
//! front broker already holds one for is never combined with it: two aggregates
//! of one period are no aggregate of the union of their instances, and an
//! aggregate is never appended to an instance list. What a source alone holds --
//! an attribute, or one more `datasetId` of an attribute -- joins the answer
//! whole. (The case that does merge instances before aggregating, a split query
//! with a value filter, lives in `crate::cons::split_temporal_settlement`.)

use crate::{
    entity::entity,
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The whole-history temporal query window: everything before 2030.
const BEFORE_2030: &str = "timerel=before&timeAt=2030-01-01T00:00:00Z";

/// The aggregation every test of this module asks for.
const AGGREGATION: &str = "format=aggregatedValues&aggrMethods=avg&aggrPeriodDuration=P1D";

/// A temporal `Property` instance of the dataset `dataset_id`, when it has one.
fn observed(value: i64, observed_at: &str, dataset_id: Option<&str>) -> Value {
    let mut instance = json!({ "type": "Property", "value": value, "observedAt": observed_at });
    if let Some(dataset_id) = dataset_id {
        instance["datasetId"] = json!(dataset_id);
    }
    instance
}

/// The aggregate objects one attribute of an aggregated answer is written as: a
/// single object for one dataset, an array of them for several.
fn aggregate_objects(attribute: &Value) -> Vec<&Value> {
    match attribute {
        Value::Array(objects) => objects.iter().collect(),
        object @ Value::Object(_) => vec![object],
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Vec::new(),
    }
}

/// The front broker holds a default-dataset `speed`; the source holds another
/// default-dataset `speed`, a `speed` of the dataset `dataset:b` and a
/// `fuelLevel`; the front broker answers the aggregation over both.
#[tokio::test]
async fn distop_aggregated_temporal_sources_merge_by_attribute_and_dataset_without_combining() {
    let topology = topology::pair().await.expect("two-broker topology");
    let entity_id = topology.id("Vehicle:aggregated-sources");
    let dataset_id = topology.id("dataset:b");
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
                    json!([observed(10, "2020-08-01T12:00:00Z", None), observed(20, "2020-08-01T13:00:00Z", None)]),
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
                &[
                    (
                        "speed",
                        json!([
                            observed(90, "2020-08-01T14:00:00Z", None),
                            observed(50, "2020-08-01T14:30:00Z", Some(&dataset_id))
                        ]),
                    ),
                    ("fuelLevel", json!([observed(70, "2020-08-01T14:00:00Z", None)])),
                ],
            ))
            .await
            .expect("holder"),
        201,
    );
    let registration = CsrBuilder::new(&topology.id("registration:aggregated-sources"), holder.peer_url())
        .tenant(topology.tenant())
        .mode("inclusive")
        .entity_id(&entity_id)
        .property_names(&["speed", "fuelLevel"])
        .operations(&["retrieveTemporal"])
        .build();
    assert_status(&front.client().create_registration(&registration).await.expect("register"), 201);

    let retrieved = front
        .client()
        .get_temporal_entity(&entity_id, &format!("{BEFORE_2030}&{AGGREGATION}"), None)
        .await
        .expect("retrieve");
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);

    let speed = aggregate_objects(&body["speed"]);
    let defaults = speed.iter().filter(|object| object.get("datasetId").is_none()).count();
    let datasets: Vec<&str> = speed.iter().filter_map(|object| object["datasetId"].as_str()).collect();
    assert_eq!(defaults, 1, "one aggregate for the default dataset, never one per holder: {body}");
    assert_eq!(
        datasets,
        vec![dataset_id.as_str()],
        "the dataset only the source holds joins the attribute: {body}"
    );
    assert!(body.get("fuelLevel").is_some(), "an attribute only the source holds joins whole: {body}");

    topology.finish().await.expect("topology cleanup");
}
