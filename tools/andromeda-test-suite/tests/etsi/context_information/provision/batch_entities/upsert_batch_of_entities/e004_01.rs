//! ETSI `004_01`: a batch upsert of entities that do not exist yet creates every one of them (clause 5.6.8).

use crate::{
    suite_keywords::{
        bare_located_building_json,
        delete_entities,
        expect_upsert_created,
        ld_json_body_client,
        query_entities_json,
        simple_building_json,
        weather_building_json,
    },
    suite_payloads::{
        BUILDING_AIR_QUALITY_WEATHER_TERMS,
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

/// Pins: Check that one can upsert a batch of non-existing entities
/// Clause: 5.6.8
#[gtest]
#[tokio::test]
async fn e004_01_01_entity_with_simple_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let entities = [building_simple_attributes(&namespace, 1)?, building_simple_attributes(&namespace, 2)?];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_upsert(&entities)
        .send()
        .await;
    expect_upsert_created(answer, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?])?;

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

/// Pins: Check that one can upsert a batch of non-existing entities
/// Clause: 5.6.8
#[gtest]
#[tokio::test]
async fn e004_01_02_entity_with_simple_relationships() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "locatedAt"]).await?;
    let entities = [building_located_in_paris(&namespace, "1")?, building_located_in_paris(&namespace, "2")?];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_upsert(&entities)
        .send()
        .await;
    expect_upsert_created(answer, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?])?;

    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            bare_located_building_json(&namespace, 1, context),
            bare_located_building_json(&namespace, 2, context)
        ]))
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can upsert a batch of non-existing entities
/// Clause: 5.6.8
#[gtest]
#[tokio::test]
async fn e004_01_03_entity_with_relationships_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_AIR_QUALITY_WEATHER_TERMS).await?;
    let entities = [building_with_weather_sensor(&namespace, 1)?, building_with_weather_sensor(&namespace, 2)?];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_upsert(&entities)
        .send()
        .await;
    expect_upsert_created(answer, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?])?;

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
