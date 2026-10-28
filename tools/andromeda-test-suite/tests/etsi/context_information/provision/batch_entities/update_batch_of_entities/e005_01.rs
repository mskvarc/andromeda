//! ETSI `005_01`: a batch of fragments updates two existing entities with simple properties, simple relationships and a
//! relationship property, and the broker retrieves the updated entities (clause 5.6.9).

use crate::{
    suite_keywords::{
        create_entity_with_context_string,
        delete_entities,
        expect_batch_done,
        ld_json_body_client,
        located_building_json,
        query_entities_json,
        simple_building_json,
        weather_building_json,
    },
    suite_payloads::{
        BUILDING_AIR_QUALITY_WEATHER_TERMS,
        BUILDING_REPLACE_TERMS,
        BUILDING_SIMPLE_TERMS,
        building_located_in_paris,
        building_simple_attributes,
        building_with_weather_sensor,
    },
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::model::NgsiLdMediaType;
use serde_json::json;

/// Pins: Check that one can update a batch of entities
/// Clause: 5.6.9
#[gtest]
#[tokio::test]
async fn e005_01_01_entity_with_simple_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 2)?).await?;
    let fragments = [
        building_simple_attributes(&namespace, 1)?.into_fragment(),
        building_simple_attributes(&namespace, 2)?.into_fragment(),
    ];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_update(&fragments)
        .send()
        .await;
    expect_batch_done(answer)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            simple_building_json(&namespace, 1, context),
            simple_building_json(&namespace, 2, context)
        ]))
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can update a batch of entities
/// Clause: 5.6.9
#[gtest]
#[tokio::test]
async fn e005_01_02_entity_with_simple_relationships() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_REPLACE_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 2)?).await?;
    let fragments = [
        building_located_in_paris(&namespace, "1")?.into_fragment(),
        building_located_in_paris(&namespace, "2")?.into_fragment(),
    ];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_update(&fragments)
        .send()
        .await;
    expect_batch_done(answer)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            located_building_json(&namespace, 1, context),
            located_building_json(&namespace, 2, context)
        ]))
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can update a batch of entities
/// Clause: 5.6.9
#[gtest]
#[tokio::test]
async fn e005_01_03_entity_with_relationships_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_AIR_QUALITY_WEATHER_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &building_simple_attributes(&namespace, 2)?).await?;
    let fragments = [
        building_with_weather_sensor(&namespace, 1)?.into_fragment(),
        building_with_weather_sensor(&namespace, 2)?.into_fragment(),
    ];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_update(&fragments)
        .send()
        .await;
    expect_batch_done(answer)?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            weather_building_json(&namespace, 1, context),
            weather_building_json(&namespace, 2, context)
        ]))
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
