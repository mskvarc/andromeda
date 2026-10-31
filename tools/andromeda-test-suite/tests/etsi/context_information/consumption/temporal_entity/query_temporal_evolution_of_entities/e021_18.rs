//! The temporal evolution of a Vehicle is queried with its attribute instances selected by `datasetId`, alone or with
//! an attribute name, and the default instance selected with `@none` (clauses 4.5.5 and 5.7.4). Each answer holds the
//! selected instances grouped by their `datasetId`, in the simplified temporal representation, and an attribute none of
//! whose instances is selected is absent from the answer.

use crate::{
    temporal_operations::{create_temporal_evolution, delete_temporal_entities},
    vehicle_entity_payloads::dataset,
    vehicle_evolution_payloads::VEHICLE_SPEED_FUEL_TERMS,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{QueryTemporalEvolution, RepresentationSpelling, SimplifiedTemporal},
    },
    model::{
        DatasetId,
        Input,
        Property,
        evolution::{TemporalAttributeName, TemporalEntity},
    },
    query::{
        SelectorType,
        temporal_query::{self, builders::TemporalQueryBuilder},
    },
};
use serde_json::{Value, json};

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-021-18-A` of the cases: a `speed` of 120, 80, 90, 60 and 62 and a `fuelLevel`
/// of 67, 68, 53 and 40, each instance on 2020-09-01 in UTC. The `speed` instances of the `Common` dataset are 80 and
/// 90, those of `Speed` are 60 and 62, and the `fuelLevel` of `Common` is 53 and the one of `fuel` is 40; the others
/// belong to the default instance.
fn vehicle_dataset_instances(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", "021-18-A")?)
        .entity_type("Vehicle")
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property(
            "speed",
            Property::builder(80)
                .observed_at("2020-09-01T12:05:00Z".parse()?)
                .dataset_id(dataset(namespace, "Common")?),
        )
        .property(
            "speed",
            Property::builder(90)
                .observed_at("2020-09-01T13:05:00Z".parse()?)
                .dataset_id(dataset(namespace, "Common")?),
        )
        .property(
            "speed",
            Property::builder(60)
                .observed_at("2020-09-01T12:05:00Z".parse()?)
                .dataset_id(dataset(namespace, "Speed")?),
        )
        .property(
            "speed",
            Property::builder(62)
                .observed_at("2020-09-01T12:15:00Z".parse()?)
                .dataset_id(dataset(namespace, "Speed")?),
        )
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(68).observed_at("2020-09-01T12:03:30Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(53)
                .observed_at("2020-09-01T13:05:00Z".parse()?)
                .dataset_id(dataset(namespace, "Common")?),
        )
        .property(
            "fuelLevel",
            Property::builder(40)
                .observed_at("2020-09-01T14:07:00Z".parse()?)
                .dataset_id(dataset(namespace, "fuel")?),
        )
        .build()?)
}

/// The simplified Query Temporal Evolution of the Vehicles in `relation`, selecting the attribute instances of `attrs`
/// and `dataset_ids`, sent without `Accept`, and the body of its 200 answer as received.
///
/// The typed reader refuses the `datasetId` that the simplified representation writes on an attribute (clause 4.5.9),
/// so the body is read as the broker sent it.
async fn selected_answer(client: &BrokerClient, relation: TemporalQueryBuilder, attrs: &[TemporalAttributeName], dataset_ids: &[DatasetId]) -> Result<Value> {
    let temporal_q = relation.build()?;
    let selector: SelectorType = "Vehicle".parse()?;
    let operation = QueryTemporalEvolution::builder()
        .temporal_q(&temporal_q)
        .entity_type(&selector)
        .attrs(attrs)
        .dataset_ids(dataset_ids)
        .spelling(RepresentationSpelling::default().in_options())
        .format(SimplifiedTemporal)
        .build();
    let answer = client.send_unchecked(client.unchecked(&operation)?).await;
    verify_that!(&answer, answered(StatusCode::OK))?;
    Ok(serde_json::from_slice(answer?.body())?)
}

/// Pins: Check that one can filter attribute instances of a temporal entity based on datasetId.
/// Clause: 4.5.5, 5.7.4
#[gtest]
#[tokio::test]
async fn e021_18_01_filter_based_on_attrs_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(&broker, &namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_dataset_instances(&namespace)?).await?;

    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = selected_answer(&client, temporal_query::after("2020-08-01T12:04:00Z".parse()?), &["speed".parse()?], &[]).await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Vehicle", "021-18-A"),
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "values": [[120, "2020-09-01T12:03:00Z"]]},
                {
                    "type": "Property",
                    "datasetId": namespace.urn_text("Dataset", "Common"),
                    "values": [[80, "2020-09-01T12:05:00Z"], [90, "2020-09-01T13:05:00Z"]],
                },
                {
                    "type": "Property",
                    "datasetId": namespace.urn_text("Dataset", "Speed"),
                    "values": [[60, "2020-09-01T12:05:00Z"], [62, "2020-09-01T12:15:00Z"]],
                },
            ],
        }]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-18-A")?]).await;
    Ok(())
}

/// Pins: Check that one can filter attribute instances of a temporal entity based on datasetId.
/// Clause: 4.5.5, 5.7.4
#[gtest]
#[tokio::test]
async fn e021_18_02_filter_based_on_dataset_id_only() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(&broker, &namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_dataset_instances(&namespace)?).await?;

    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = selected_answer(
        &client,
        temporal_query::after("2020-08-01T12:04:00Z".parse()?),
        &[],
        &[dataset(&namespace, "Common")?],
    )
    .await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Vehicle", "021-18-A"),
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "datasetId": namespace.urn_text("Dataset", "Common"),
                "values": [[80, "2020-09-01T12:05:00Z"], [90, "2020-09-01T13:05:00Z"]],
            },
            "fuelLevel": {
                "type": "Property",
                "datasetId": namespace.urn_text("Dataset", "Common"),
                "values": [[53, "2020-09-01T13:05:00Z"]],
            },
        }]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-18-A")?]).await;
    Ok(())
}

/// Pins: Check that one can filter attribute instances of a temporal entity based on datasetId.
/// Clause: 4.5.5, 5.7.4
#[gtest]
#[tokio::test]
async fn e021_18_03_filter_based_on_two_dataset_ids() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(&broker, &namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_dataset_instances(&namespace)?).await?;

    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = selected_answer(
        &client,
        temporal_query::after("2020-08-01T12:04:00Z".parse()?),
        &[],
        &[dataset(&namespace, "Common")?, dataset(&namespace, "Speed")?],
    )
    .await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Vehicle", "021-18-A"),
            "type": "Vehicle",
            "speed": [
                {
                    "type": "Property",
                    "datasetId": namespace.urn_text("Dataset", "Common"),
                    "values": [[80, "2020-09-01T12:05:00Z"], [90, "2020-09-01T13:05:00Z"]],
                },
                {
                    "type": "Property",
                    "datasetId": namespace.urn_text("Dataset", "Speed"),
                    "values": [[60, "2020-09-01T12:05:00Z"], [62, "2020-09-01T12:15:00Z"]],
                },
            ],
            "fuelLevel": {
                "type": "Property",
                "datasetId": namespace.urn_text("Dataset", "Common"),
                "values": [[53, "2020-09-01T13:05:00Z"]],
            },
        }]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-18-A")?]).await;
    Ok(())
}

/// Pins: Check that one can filter attribute instances of a temporal entity based on datasetId.
/// Clause: 4.5.5, 5.7.4
#[gtest]
#[tokio::test]
async fn e021_18_04_filter_based_on_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(&broker, &namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_dataset_instances(&namespace)?).await?;

    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = selected_answer(
        &client,
        temporal_query::after("2020-08-01T12:04:00Z".parse()?),
        &[],
        &[DatasetId::DefaultInstance],
    )
    .await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Vehicle", "021-18-A"),
            "type": "Vehicle",
            "speed": {"type": "Property", "values": [[120, "2020-09-01T12:03:00Z"]]},
            "fuelLevel": {
                "type": "Property",
                "values": [[67, "2020-09-01T12:03:00Z"], [68, "2020-09-01T12:03:30Z"]],
            },
        }]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-18-A")?]).await;
    Ok(())
}

/// Pins: Check that one can filter attribute instances of a temporal entity based on datasetId.
/// Clause: 4.5.5, 5.7.4
#[gtest]
#[tokio::test]
async fn e021_18_05_filter_based_on_attrs_and_default_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(&broker, &namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_dataset_instances(&namespace)?).await?;

    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = selected_answer(
        &client,
        temporal_query::after("2020-08-01T12:04:00Z".parse()?),
        &["speed".parse()?],
        &[DatasetId::DefaultInstance],
    )
    .await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Vehicle", "021-18-A"),
            "type": "Vehicle",
            "speed": {"type": "Property", "values": [[120, "2020-09-01T12:03:00Z"]]},
        }]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-18-A")?]).await;
    Ok(())
}

/// Pins: Check that one can filter attribute instances of a temporal entity based on datasetId.
/// Clause: 4.5.5, 5.7.4
#[gtest]
#[tokio::test]
async fn e021_18_06_filter_based_on_attrs_and_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(&broker, &namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_dataset_instances(&namespace)?).await?;

    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = selected_answer(
        &client,
        temporal_query::after("2020-08-01T12:04:00Z".parse()?),
        &["fuelLevel".parse()?],
        &[dataset(&namespace, "fuel")?],
    )
    .await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Vehicle", "021-18-A"),
            "type": "Vehicle",
            "fuelLevel": {
                "type": "Property",
                "datasetId": namespace.urn_text("Dataset", "fuel"),
                "values": [[40, "2020-09-01T14:07:00Z"]],
            },
        }]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-18-A")?]).await;
    Ok(())
}

/// Pins: Check that one can filter attribute instances of a temporal entity based on datasetId.
/// Clause: 4.5.5, 5.7.4
/// Expected: the Vehicle is answered without attributes when no selected instance carries the datasetId (clause
/// 5.7.4.4)
#[gtest]
#[tokio::test]
async fn e021_18_07_filter_based_on_attrs_and_dataset_id_with_no_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_temporal_evolution(&broker, &namespace, &VEHICLE_SPEED_FUEL_TERMS, &vehicle_dataset_instances(&namespace)?).await?;

    let client = broker.with_context(vocabulary.context())?.with_accept(Accept::any());
    let body = selected_answer(
        &client,
        temporal_query::after("2020-08-01T12:04:00Z".parse()?),
        &["speed".parse()?],
        &[dataset(&namespace, "fuel")?],
    )
    .await?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("Vehicle", "021-18-A"),
            "type": "Vehicle",
        }]))
        .hiding("instanceId")
    );

    delete_temporal_entities(&broker, &[&namespace.urn("Vehicle", "021-18-A")?]).await;
    Ok(())
}
