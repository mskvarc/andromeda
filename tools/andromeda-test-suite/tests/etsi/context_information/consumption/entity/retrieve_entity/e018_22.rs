//! An entity with a multivalued relationship is retrieved, normalized or simplified, with its linked entities joined
//! inline or flat (clauses 5.7.1 and 4.5.23).

use crate::{
    entity_operations::{create_entity, delete_entities},
    linked_place_payloads::city,
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
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{InlineJoin, RepresentationSpelling},
    },
    model::{Entity, EntityId, Relationship},
};
use serde_json::json;
use std::num::NonZeroU32;

/// The terms of the `@context` the multivalued cases register.
const CITY_TERMS: [&str; 3] = ["Building", "locatedAt", "City"];

/// The identifiers of the case's Building, Paris and Lyon.
fn identifiers(namespace: &Namespace) -> Result<[EntityId; 3]> {
    Ok([
        namespace.urn("Building", "018-22")?,
        namespace.urn("City", "Paris")?,
        namespace.urn("City", "Lyon")?,
    ])
}

/// Creates the case's Building, whose `locatedAt` Relationship targets the Cities Paris and Lyon, in `vocabulary`'s
/// context.
async fn create_building_located_at_both(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    let located_at = Relationship::builder(vec![namespace.urn("City", "Paris")?, namespace.urn("City", "Lyon")?]);
    let building = Entity::builder()
        .id(namespace.urn("Building", "018-22")?)
        .entity_type("Building")
        .relationship("locatedAt", located_at)
        .build()?;
    create_entity(broker, vocabulary, &building).await
}

/// Registers the case's `@context`, creates its Building and both Cities in it, and answers the context.
async fn created(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &CITY_TERMS).await?;
    create_building_located_at_both(broker, &vocabulary, namespace).await?;
    create_entity(broker, &vocabulary, &city(namespace, "Paris")?).await?;
    create_entity(broker, &vocabulary, &city(namespace, "Lyon")?).await?;
    Ok(vocabulary)
}

/// Pins: Check that an entity with a multivalued relationship can be retrieved
/// Clause: 5.7.1
#[gtest]
#[tokio::test]
async fn e018_22_01_retrieve_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let [building, paris, lyon] = identifiers(&namespace)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve(&building)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.to_string(),
        "type": "Building",
        "locatedAt": {
            "type": "Relationship",
            "object": [paris.to_string(), lyon.to_string()],
        },
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&building, &paris, &lyon]).await;
    Ok(())
}

/// Pins: Check that an entity with a multivalued relationship can be retrieved in simplified representation
/// Clause: 5.7.1
#[gtest]
#[tokio::test]
async fn e018_22_02_retrieve_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let [building, paris, lyon] = identifiers(&namespace)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve_simplified(&building)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.to_string(),
        "type": "Building",
        "locatedAt": [paris.to_string(), lyon.to_string()],
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&building, &paris, &lyon]).await;
    Ok(())
}

/// Pins: Check that an entity with a multivalued relationship can be retrieved with inline linked entities
/// Clause: 5.7.1, 4.5.23
#[gtest]
#[tokio::test]
async fn e018_22_03_retrieve_normalized_with_inline_linked_entity_retrieval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let [building, paris, lyon] = identifiers(&namespace)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
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
            "object": [paris.to_string(), lyon.to_string()],
            "entity": [
                {"id": paris.to_string(), "type": "City"},
                {"id": lyon.to_string(), "type": "City"},
            ],
        },
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&building, &paris, &lyon]).await;
    Ok(())
}

/// Pins: Check that an entity with a multivalued relationship can be retrieved with flat linked entities
/// Clause: 5.7.1, 4.5.23
#[gtest]
#[tokio::test]
async fn e018_22_04_retrieve_normalized_with_flat_linked_entity_retrieval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let [building, paris, lyon] = identifiers(&namespace)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
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
            "locatedAt": {
                "type": "Relationship",
                "object": [paris.to_string(), lyon.to_string()],
            },
        },
        {"id": paris.to_string(), "type": "City"},
        {"id": lyon.to_string(), "type": "City"},
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&building, &paris, &lyon]).await;
    Ok(())
}

/// Pins: Check that an entity with a multivalued relationship can be retrieved with inline linked entities in
/// simplified representation
/// Clause: 5.7.1, 4.5.23
#[gtest]
#[tokio::test]
async fn e018_22_05_retrieve_simplified_with_inline_linked_entity_retrieval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let [building, paris, lyon] = identifiers(&namespace)?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve_simplified(&building)
        .join(InlineJoin::Inline)
        .join_level(NonZeroU32::MIN)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.to_string(),
        "type": "Building",
        "locatedAt": [
            {"id": paris.to_string(), "type": "City"},
            {"id": lyon.to_string(), "type": "City"},
        ],
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&building, &paris, &lyon]).await;
    Ok(())
}
