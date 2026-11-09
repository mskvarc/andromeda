//! An entity is retrieved as a `GeoJSON` Feature, in its normalized and simplified forms and with the geometry property
//! the request names (clause 6.3.7).

use crate::entity_operations::create_entity;
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
use ngsi_ld::model::{
    Entity,
    Input,
    geometry::{Position, Shape},
};
use serde_json::{Map, json};

/// The terms of the case's `@context`.
const TERMS: [&str; 3] = ["Building", "name", "subCategory"];

/// A point at `longitude` and `latitude`, as a geometry value.
fn point(longitude: f64, latitude: f64) -> Result<Shape> {
    Ok(Shape::Point(Position::try_new(longitude, latitude)?))
}

/// The case's building: a name, a sub-category, a `location` and an `observationSpace`, both points.
fn geo_building(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .geo_property("location", point(13.3986, 52.5547)?)
        .geo_property("observationSpace", point(11.1111, 11.1111)?)
        .build()?)
}

/// Pins: Check that the queried entity by id can be returned in a `GeoJSON` format
/// Clause: 6.3.7
#[gtest]
#[tokio::test]
async fn e018_05_01_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = geo_building(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .retrieve_geo_json_simplified(&building.id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": "Feature",
        "geometry": {"type": "Point", "coordinates": [13.3986, 52.5547]},
        "properties": {
            "type": "Building",
            "name": "Eiffel Tower",
            "subCategory": "tourism",
            "location": {"type": "Point", "coordinates": [13.3986, 52.5547]},
            "observationSpace": {"type": "Point", "coordinates": [11.1111, 11.1111]},
        },
        "@context": vocabulary.url().as_str(),
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(5)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that the queried entity by id can be returned in a `GeoJSON` format
/// Clause: 6.3.7
#[gtest]
#[tokio::test]
async fn e018_05_02_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = geo_building(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .retrieve_geo_json(&building.id)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": "Feature",
        "geometry": {"type": "Point", "coordinates": [13.3986, 52.5547]},
        "properties": {
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}},
            "observationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [11.1111, 11.1111]}},
        },
        "@context": vocabulary.url().as_str(),
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(5)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that the queried entity by id can be returned in a `GeoJSON` format
/// Clause: 6.3.7
#[gtest]
#[tokio::test]
async fn e018_05_03_with_geometry_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = geo_building(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let geometry_property = "observationSpace".parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .retrieve_geo_json(&building.id)
        .geometry_property(&geometry_property)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": "Feature",
        "geometry": {"type": "Point", "coordinates": [11.1111, 11.1111]},
        "properties": {
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}},
            "observationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [11.1111, 11.1111]}},
        },
        "@context": vocabulary.url().as_str(),
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(5)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that the queried entity by id can be returned in a `GeoJSON` format
/// Clause: 6.3.7
#[gtest]
#[tokio::test]
async fn e018_05_04_with_nonexistent_geometry_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = geo_building(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let geometry_property = "operationSpace".parse()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .retrieve_geo_json(&building.id)
        .geometry_property(&geometry_property)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.id.to_string(),
        "type": "Feature",
        "geometry": null,
        "properties": {
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}},
            "observationSpace": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [11.1111, 11.1111]}},
        },
        "@context": vocabulary.url().as_str(),
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(5)));
    broker.entities().delete(&building.id).send().await.any_answer();
    broker.temporal().delete(&building.id).send().await.any_answer();
    Ok(())
}
