//! An entity with a geospatial Property named `operationSpace`, a Polygon, is retrieved in the simplified and the
//! normalized representation (clauses 5.7.1 and 4.7).

use crate::{entity_operations::create_entity, geometry_payloads::polygon};
use andromeda_test_suite::{
    broker,
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{Entity, Input};
use serde_json::{Map, Value, json};

/// The terms of the case's `@context`.
const TERMS: [&str; 1] = ["Building"];

/// The case's square, the ring of five positions the case stores.
const SQUARE: [(f64, f64); 5] = [(100.12, 0.23), (101.12, 0.23), (101.12, 1.23), (100.12, 1.23), (100.12, 0.23)];

/// The case's building, with an `operationSpace` Polygon.
fn operation_space_building(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .geo_property("operationSpace", polygon(&SQUARE)?)
        .build()?)
}

/// The case's ring as the JSON coordinates of a Polygon.
fn ring_coordinates() -> Value {
    json!([[[100.12, 0.23], [101.12, 0.23], [101.12, 1.23], [100.12, 1.23], [100.12, 0.23]]])
}

/// Pins: Check that an entity with operationSpace geospatial Property can be retrieved
/// Clause: 5.7.1, 4.7
/// Expected: the ring is compared as stored, its repeated closing corner counted
#[gtest]
#[tokio::test]
async fn e018_09_01_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = operation_space_building(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .retrieve_simplified(&building.id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": "Building",
        "operationSpace": {"type": "Polygon", "coordinates": ring_coordinates()},
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(3)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that an entity with operationSpace geospatial Property can be retrieved
/// Clause: 5.7.1, 4.7
/// Expected: the ring is compared as stored, its repeated closing corner counted
#[gtest]
#[tokio::test]
async fn e018_09_02_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = operation_space_building(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .retrieve(&building.id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": "Building",
        "operationSpace": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": ring_coordinates()}},
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(3)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
