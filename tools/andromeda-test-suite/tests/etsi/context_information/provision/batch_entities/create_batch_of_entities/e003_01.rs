//! ETSI `003_01`: a batch of entities with a minimal body, simple properties, a relationship and a relationship
//! property is created, and the broker retrieves the entities as written (clause 5.6.7).

use crate::{
    suite_keywords::{batch_create, delete_entities, ld_json_body_client, query_entities_json, simple_building_json, weather_building_json},
    suite_payloads::{
        BUILDING_AIR_QUALITY_WEATHER_TERMS,
        BUILDING_SIMPLE_TERMS,
        bare_building,
        building_located_in_paris,
        building_simple_attributes,
        building_with_weather_sensor,
    },
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// Pins: Check that one can create a batch of entities
/// Clause: 5.6.7
#[gtest]
#[tokio::test]
async fn e003_01_01_minimal_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let entities = [bare_building(&namespace, 1)?, bare_building(&namespace, 2)?];
    let (first, second) = (namespace.urn("Building", 1)?, namespace.urn("Building", 2)?);
    let created = batch_create(&ld_json_body_client(&broker, &vocabulary)?, &entities).await?;
    expect_that!(&created, unordered_elements_are![eq(&first), eq(&second)]);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    expect_that!(
        &body,
        matches_document(json!([
            {"id": namespace.urn_text("Building", 1), "type": "Building", "@context": vocabulary.url().as_str()},
            {"id": namespace.urn_text("Building", 2), "type": "Building", "@context": vocabulary.url().as_str()},
        ]))
    );
    delete_entities(&broker, &[&first, &second]).await;
    Ok(())
}

/// Pins: Check that one can create a batch of entities
/// Clause: 5.6.7
#[gtest]
#[tokio::test]
async fn e003_01_02_entity_with_simple_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entities = [building_simple_attributes(&namespace, 1)?, building_simple_attributes(&namespace, 2)?];
    let (first, second) = (namespace.urn("Building", 1)?, namespace.urn("Building", 2)?);
    let created = batch_create(&ld_json_body_client(&broker, &vocabulary)?, &entities).await?;
    expect_that!(&created, unordered_elements_are![eq(&first), eq(&second)]);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            simple_building_json(&namespace, 1, context),
            simple_building_json(&namespace, 2, context),
        ]))
    );
    delete_entities(&broker, &[&first, &second]).await;
    Ok(())
}

/// Pins: Check that one can create a batch of entities
/// Clause: 5.6.7
#[gtest]
#[tokio::test]
async fn e003_01_03_entity_with_simple_relationships() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "locatedAt"]).await?;
    let entities = [building_located_in_paris(&namespace, "1")?, building_located_in_paris(&namespace, "2")?];
    let (first, second) = (namespace.urn("Building", 1)?, namespace.urn("Building", 2)?);
    let created = batch_create(&ld_json_body_client(&broker, &vocabulary)?, &entities).await?;
    expect_that!(&created, unordered_elements_are![eq(&first), eq(&second)]);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let located_at = namespace.urn_text("City", "Paris");
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", 1),
                "type": "Building",
                "locatedAt": {"type": "Relationship", "object": located_at},
                "@context": vocabulary.url().as_str(),
            },
            {
                "id": namespace.urn_text("Building", 2),
                "type": "Building",
                "locatedAt": {"type": "Relationship", "object": located_at},
                "@context": vocabulary.url().as_str(),
            },
        ]))
    );
    delete_entities(&broker, &[&first, &second]).await;
    Ok(())
}

/// Pins: Check that one can create a batch of entities
/// Clause: 5.6.7
#[gtest]
#[tokio::test]
async fn e003_01_04_entity_with_relationships_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_AIR_QUALITY_WEATHER_TERMS).await?;
    let entities = [building_with_weather_sensor(&namespace, 1)?, building_with_weather_sensor(&namespace, 2)?];
    let (first, second) = (namespace.urn("Building", 1)?, namespace.urn("Building", 2)?);
    let created = batch_create(&ld_json_body_client(&broker, &vocabulary)?, &entities).await?;
    expect_that!(&created, unordered_elements_are![eq(&first), eq(&second)]);
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            weather_building_json(&namespace, 1, context),
            weather_building_json(&namespace, 2, context),
        ]))
    );
    delete_entities(&broker, &[&first, &second]).await;
    Ok(())
}
