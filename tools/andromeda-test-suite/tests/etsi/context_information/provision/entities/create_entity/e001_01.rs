//! An entity with a minimal body, simple properties, relationships, geo properties and a multi-valued relationship is
//! created, and the broker retrieves it as written (clause 5.6.1).

use crate::{
    building_payloads::{BUILDING_SIMPLE_TERMS, bare_building, building_simple_attributes, building_with_weather_sensor, eiffel_tower_located},
    entity_operations::{create_entity_answer, create_entity_plain, delete_entities, expect_created, retrieve_entity_json},
    geometry_payloads::point,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Input, NgsiLdMediaType, Relationship};
use serde_json::json;

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose only attribute is the `GeoProperty` `customLocation`, at the point
/// 13.3986, 52.5547.
fn building_custom_location(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .geo_property("customLocation", point(13.3986, 52.5547)?)
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose `locatedAt` Relationship holds the two Cities `<ns>-Paris` and
/// `<ns>-Lyon`.
fn building_located_at_two(namespace: &Namespace) -> Result<Entity<Input>> {
    let located_at = Relationship::builder(vec![namespace.urn("City", "Paris")?, namespace.urn("City", "Lyon")?]);
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .relationship("locatedAt", located_at)
        .build()?)
}

/// Pins: Check that one can create an entity
/// Clause: 5.6.1
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e001_01_01_minimal_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let entity = bare_building(&namespace, 1)?;
    let created = create_entity_plain(&broker, &entity).await?;
    expect_created(&created, &entity.id);
    let body = retrieve_entity_json(&broker.with_accept(NgsiLdMediaType::Json), &entity.id).await?;
    expect_that!(&body, matches_document(json!({"id": entity.id.to_string(), "type": "Building"})));
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}

/// Pins: Check that one can create an entity
/// Clause: 5.6.1
#[gtest]
#[tokio::test]
async fn e001_01_02_entity_with_simple_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entity = building_simple_attributes(&namespace, 1)?;
    let created = create_entity_answer(&broker, &vocabulary, &entity).await?;
    expect_created(&created, &entity.id);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "airQualityLevel": {
                "type": "Property",
                "value": 4,
                "unitCode": "C62",
                "observedAt": "2020-09-09T16:40:00.000Z"
            },
            "almostFull": {"type": "Property", "value": false},
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}

/// Pins: Check that one can create an entity
/// Clause: 5.6.1
#[gtest]
#[tokio::test]
async fn e001_01_03_entity_with_relationships_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let terms = ["Building", "name", "subCategory", "airQualityLevel", "hasWeatherObserved", "almostFull"];
    let vocabulary = Vocabulary::register(&broker, &namespace, &terms).await?;
    let entity = building_with_weather_sensor(&namespace, 1)?;
    let created = create_entity_answer(&broker, &vocabulary, &entity).await?;
    expect_created(&created, &entity.id);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    let sensor = namespace.urn_text("Sensor", "43a1af27-4aa7-44fe-bee9-fe76d56978a4");
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "airQualityLevel": {
                "type": "Property",
                "value": 4,
                "unitCode": "C62",
                "observedAt": "2020-09-09T16:40:00.000Z",
                "hasWeatherObserved": {"type": "Relationship", "object": sensor}
            },
            "almostFull": {"type": "Property", "value": false},
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}

/// Pins: Check that one can create an entity
/// Clause: 5.6.1
#[gtest]
#[tokio::test]
async fn e001_01_04_entity_with_location_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "name", "subCategory"]).await?;
    let entity = eiffel_tower_located(&namespace, 1)?;
    let created = create_entity_answer(&broker, &vocabulary, &entity).await?;
    expect_created(&created, &entity.id);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
            },
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}

/// Pins: Check that one can create an entity
/// Clause: 5.6.1
#[gtest]
#[tokio::test]
async fn e001_01_05_entity_with_non_core_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "customLocation"]).await?;
    let entity = building_custom_location(&namespace)?;
    let created = create_entity_answer(&broker, &vocabulary, &entity).await?;
    expect_created(&created, &entity.id);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "customLocation": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}
            },
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}

/// Pins: Check that one can create an entity
/// Clause: 5.6.1
#[gtest]
#[tokio::test]
async fn e001_01_06_entity_with_multivalued_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "locatedAt"]).await?;
    let entity = building_located_at_two(&namespace)?;
    let created = create_entity_answer(&broker, &vocabulary, &entity).await?;
    expect_created(&created, &entity.id);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &entity.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": entity.id.to_string(),
            "type": "Building",
            "locatedAt": {
                "type": "Relationship",
                "object": [namespace.urn_text("City", "Paris"), namespace.urn_text("City", "Lyon")]
            },
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&entity.id]).await;
    Ok(())
}
