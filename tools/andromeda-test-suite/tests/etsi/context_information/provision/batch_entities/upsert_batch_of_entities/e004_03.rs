//! ETSI `004_03`: a batch upsert of entities that all exist replaces each of them, with the scope or the types the batch
//! carries (clauses 4.16, 4.18 and 5.6.8).

use crate::{
    suite_keywords::{
        bare_building_json,
        bare_located_building_json,
        create_entity_with_context_string,
        delete_entities,
        expect_batch_done,
        ld_json_body_client,
        query_entities_json,
        simple_building_json,
        typed_building_json,
        weather_building_json,
    },
    suite_payloads::{
        BUILDING_AIR_QUALITY_WEATHER_TERMS,
        BUILDING_SIMPLE_TERMS,
        bare_building,
        building_located_in_paris,
        building_of_types,
        building_simple_attributes,
        building_with_weather_sensor,
    },
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::UpsertMode,
    model::{Entity, Input, NgsiLdMediaType},
};
use serde_json::json;

/// The Building `urn:ngsi-ld:Building:<ns>-<n>` scoped to `/Madrid/Gardens/ParqueNorte`.
fn scoped_building(namespace: &Namespace, n: u32) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", n)?)
        .entity_type("Building")
        .scope("/Madrid/Gardens/ParqueNorte")
        .build()?)
}

/// Pins: Check that one can upsert a batch of existing entities
/// Clause: 5.6.8
#[gtest]
#[tokio::test]
async fn e004_03_01_entity_with_simple_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let entities = [building_simple_attributes(&namespace, 1)?, building_simple_attributes(&namespace, 2)?];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_upsert(&entities)
        .mode(UpsertMode::Replace)
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

/// Pins: Check that one can upsert a batch of existing entities
/// Clause: 5.6.8
#[gtest]
#[tokio::test]
async fn e004_03_02_entity_with_simple_relationships() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "locatedAt"]).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let entities = [building_located_in_paris(&namespace, "1")?, building_located_in_paris(&namespace, "2")?];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_upsert(&entities)
        .mode(UpsertMode::Replace)
        .send()
        .await;
    expect_batch_done(answer)?;

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

/// Pins: Check that one can upsert a batch of existing entities
/// Clause: 5.6.8
#[gtest]
#[tokio::test]
async fn e004_03_03_entity_with_relationships_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_AIR_QUALITY_WEATHER_TERMS).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let entities = [building_with_weather_sensor(&namespace, 1)?, building_with_weather_sensor(&namespace, 2)?];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_upsert(&entities)
        .mode(UpsertMode::Replace)
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

/// Pins: Check that one can upsert a batch of existing entities
/// Clause: 4.18, 5.6.8
#[gtest]
#[tokio::test]
async fn e004_03_04_entity_with_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let entities = [scoped_building(&namespace, 1)?, scoped_building(&namespace, 2)?];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_upsert(&entities)
        .mode(UpsertMode::Replace)
        .send()
        .await;
    expect_batch_done(answer)?;

    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    let scoped = |n: u32| {
        let mut building = bare_building_json(&namespace, n, context);
        building["scope"] = json!("/Madrid/Gardens/ParqueNorte");
        building
    };
    expect_that!(&body, matches_document(json!([scoped(1), scoped(2)])));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can upsert a batch of existing entities
/// Clause: 4.16, 5.6.8
#[gtest]
#[tokio::test]
async fn e004_03_05_entity_with_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building", "TouristDestination"]).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let entities = [
        building_of_types(&namespace, 1, ["Building", "TouristDestination"])?,
        building_of_types(&namespace, 2, ["Building", "TouristDestination"])?,
    ];
    let answer = ld_json_body_client(&broker, &vocabulary)?
        .with_responses()
        .entities()
        .batch_upsert(&entities)
        .mode(UpsertMode::Replace)
        .send()
        .await;
    expect_batch_done(answer)?;

    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = query_entities_json(&client, vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?], "Building").await?;
    let context = vocabulary.url().as_str();
    expect_that!(
        &body,
        matches_document(json!([
            typed_building_json(&namespace, 1, &["Building", "TouristDestination"], context),
            typed_building_json(&namespace, 2, &["Building", "TouristDestination"], context)
        ]))
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
