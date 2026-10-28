//! ETSI `018_08`: an entity with a geospatial Property named `observationSpace` is retrieved in the simplified and the
//! normalized representation (clauses 5.7.1 and 4.7).

use crate::suite_keywords::create_entity;
use andromeda_test_suite::{
    broker::{self},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{
    Entity,
    Input,
    geometry::{Position, Shape},
};
use serde_json::{Map, json};

/// The terms of the case's `@context`.
const TERMS: [&str; 1] = ["Building"];

/// The case's building, with an `observationSpace` point at 13.3986, 52.5547.
fn observation_space_building(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .geo_property("observationSpace", Shape::Point(Position::try_new(13.3986, 52.5547)?))
        .build()?)
}

/// Pins: Check that an entity with observationSpace geospatial Property can be retrieved
/// Clause: 5.7.1, 4.7
#[gtest]
#[tokio::test]
async fn e018_08_01_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = observation_space_building(&namespace)?;
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
        "observationSpace": {"type": "Point", "coordinates": [13.3986, 52.5547]},
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(3)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that an entity with observationSpace geospatial Property can be retrieved
/// Clause: 5.7.1, 4.7
#[gtest]
#[tokio::test]
async fn e018_08_02_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = observation_space_building(&namespace)?;
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
        "observationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}},
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(3)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
