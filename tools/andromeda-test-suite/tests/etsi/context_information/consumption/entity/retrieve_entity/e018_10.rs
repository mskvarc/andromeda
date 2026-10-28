//! ETSI `018_10`: an entity whose Relationship targets a linked entity is retrieved with that entity inline or flat, in
//! the normalized and the simplified representation (clauses 5.7.1 and 4.5.23).

use crate::{
    suite_keywords::{create_entity, delete_entities},
    suite_payloads::{building_located_at, city_paris},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::http::operation::InlineJoin;
use serde_json::{Map, json};
use std::num::NonZeroU32;

/// The terms of the case's `@context`.
const TERMS: [&str; 3] = ["Building", "locatedAt", "City"];

/// Creates the case's Building, located at the City, and the City, in `vocabulary`'s context.
async fn created_pair(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    create_entity(broker, vocabulary, &building_located_at(namespace, "018-10")?).await?;
    create_entity(broker, vocabulary, &city_paris(namespace)?).await
}

/// Pins: Check that an entity can be retrieved with a linked entity in different join types and representations
/// Clause: 5.7.1, 4.5.23
/// Corrected: fix(linked-objecttype): linked-entity retrieval expectations carry objecttype (table 5.2.6-1)
#[gtest]
#[tokio::test]
async fn e018_10_01_inline_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    created_pair(&broker, &vocabulary, &namespace).await?;
    let building = namespace.urn("Building", "018-10")?;
    let city = namespace.urn("City", "Paris")?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .retrieve(&building)
        .join(InlineJoin::Inline)
        .join_level(NonZeroU32::MIN)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.to_string(),
        "type": "Building",
        "locatedAt": {
            "type": "Relationship",
            "object": city.to_string(),
            "objectType": "City",
            "entity": {"id": city.to_string(), "type": "City"},
        },
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(3)));
    delete_entities(&broker, &[&building, &city]).await;
    Ok(())
}

/// Pins: Check that an entity can be retrieved with a linked entity in different join types and representations
/// Clause: 5.7.1, 4.5.23
/// Corrected: fix(linked-objecttype): linked-entity retrieval expectations carry objecttype (table 5.2.6-1)
#[gtest]
#[tokio::test]
async fn e018_10_02_flat_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    created_pair(&broker, &vocabulary, &namespace).await?;
    let building = namespace.urn("Building", "018-10")?;
    let city = namespace.urn("City", "Paris")?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .retrieve_flattened(&building)
        .join_level(NonZeroU32::MIN)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {
            "id": building.to_string(),
            "type": "Building",
            "locatedAt": {"type": "Relationship", "object": city.to_string(), "objectType": "City"},
        },
        {"id": city.to_string(), "type": "City"},
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&building, &city]).await;
    Ok(())
}

/// Pins: Check that an entity can be retrieved with a linked entity in different join types and representations
/// Clause: 5.7.1, 4.5.23
#[gtest]
#[tokio::test]
async fn e018_10_03_inline_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    created_pair(&broker, &vocabulary, &namespace).await?;
    let building = namespace.urn("Building", "018-10")?;
    let city = namespace.urn("City", "Paris")?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .retrieve_simplified(&building)
        .join(InlineJoin::Inline)
        .join_level(NonZeroU32::MIN)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.to_string(),
        "type": "Building",
        "locatedAt": {"id": city.to_string(), "type": "City"},
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    expect_that!(body.as_object().map(Map::len), some(eq(3)));
    delete_entities(&broker, &[&building, &city]).await;
    Ok(())
}

/// Pins: Check that an entity can be retrieved with a linked entity in different join types and representations
/// Clause: 5.7.1, 4.5.23
#[gtest]
#[tokio::test]
async fn e018_10_04_flat_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    created_pair(&broker, &vocabulary, &namespace).await?;
    let building = namespace.urn("Building", "018-10")?;
    let city = namespace.urn("City", "Paris")?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .entities()
        .retrieve_flattened_simplified(&building)
        .join_level(NonZeroU32::MIN)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {"id": building.to_string(), "type": "Building", "locatedAt": city.to_string()},
        {"id": city.to_string(), "type": "City"},
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&building, &city]).await;
    Ok(())
}
