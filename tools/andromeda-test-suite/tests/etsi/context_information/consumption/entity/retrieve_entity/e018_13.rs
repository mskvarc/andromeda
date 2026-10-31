//! An entity is retrieved with two levels of linked entities, inline or flat, in the normalized and the simplified
//! representation (clauses 5.7.1 and 4.5.23).

use crate::{
    entity_operations::{create_entity, delete_entities},
    linked_place_payloads::building_located_at,
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
    model::{Entity, Input, NgsiLdMediaType, Relationship, VocabTerm},
};
use serde_json::{Value, json};
use std::num::NonZeroU32;

/// The terms of the case's `@context`.
const TERMS: [&str; 4] = ["Building", "locatedAt", "City", "Country"];

/// The City `urn:ngsi-ld:City:<ns>-Paris`, located at the Country `urn:ngsi-ld:Country:<ns>-France`.
fn city_located_in_france(namespace: &Namespace) -> Result<Entity<Input>> {
    let located_at = Relationship::builder(namespace.urn("Country", "France")?).object_type("Country".parse::<VocabTerm>()?);
    Ok(Entity::builder()
        .id(namespace.urn("City", "Paris")?)
        .entity_type("City")
        .relationship("locatedAt", located_at)
        .build()?)
}

/// The Country `urn:ngsi-ld:Country:<ns>-France`, with no attribute.
fn country_france(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder().id(namespace.urn("Country", "France")?).entity_type("Country").build()?)
}

/// The normalized Building with its two levels of linked entities, joined inline.
fn inline_normalized(namespace: &Namespace) -> Value {
    json!({
        "id": namespace.urn_text("Building", "018-13"),
        "type": "Building",
        "locatedAt": {
            "type": "Relationship",
            "object": namespace.urn_text("City", "Paris"),
            "objectType": "City",
            "entity": {
                "id": namespace.urn_text("City", "Paris"),
                "type": "City",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("Country", "France"),
                    "objectType": "Country",
                    "entity": {"id": namespace.urn_text("Country", "France"), "type": "Country"},
                },
            },
        },
    })
}

/// The normalized Building and its linked entities, joined flat.
fn flat_normalized(namespace: &Namespace) -> Value {
    json!([
        {
            "id": namespace.urn_text("Building", "018-13"),
            "type": "Building",
            "locatedAt": {
                "type": "Relationship",
                "object": namespace.urn_text("City", "Paris"),
                "objectType": "City",
            },
        },
        {
            "id": namespace.urn_text("City", "Paris"),
            "type": "City",
            "locatedAt": {
                "type": "Relationship",
                "object": namespace.urn_text("Country", "France"),
                "objectType": "Country",
            },
        },
        {"id": namespace.urn_text("Country", "France"), "type": "Country"},
    ])
}

/// The simplified Building with its two levels of linked entities, joined inline.
fn inline_simplified(namespace: &Namespace) -> Value {
    json!({
        "id": namespace.urn_text("Building", "018-13"),
        "type": "Building",
        "locatedAt": {
            "id": namespace.urn_text("City", "Paris"),
            "type": "City",
            "locatedAt": {"id": namespace.urn_text("Country", "France"), "type": "Country"},
        },
    })
}

/// The simplified Building and its linked entities, joined flat.
fn flat_simplified(namespace: &Namespace) -> Value {
    json!([
        {
            "id": namespace.urn_text("Building", "018-13"),
            "type": "Building",
            "locatedAt": namespace.urn_text("City", "Paris"),
        },
        {
            "id": namespace.urn_text("City", "Paris"),
            "type": "City",
            "locatedAt": namespace.urn_text("Country", "France"),
        },
        {"id": namespace.urn_text("Country", "France"), "type": "Country"},
    ])
}

/// Creates the case's Building, City and Country in `vocabulary`'s context, and returns the Building.
async fn create_linked(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<Entity<Input>> {
    let building = building_located_at(namespace, "018-13")?;
    create_entity(broker, vocabulary, &building).await?;
    create_entity(broker, vocabulary, &city_located_in_france(namespace)?).await?;
    create_entity(broker, vocabulary, &country_france(namespace)?).await?;
    Ok(building)
}

/// Pins: Check that an entity can be retrieved with two levels linked entities in different join types and
/// representations
/// Clause: 5.7.1, 4.5.23
/// Expected: linked-entity retrieval answers carry objectType (Table 5.2.6-1)
#[gtest]
#[tokio::test]
async fn e018_13_01_inline_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = create_linked(&broker, &vocabulary, &namespace).await?;
    let client = broker.with_context(vocabulary.context())?;
    let received = client
        .with_responses()
        .entities()
        .retrieve(&building.id)
        .join(InlineJoin::Inline)
        .join_level(NonZeroU32::try_from(2)?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(inline_normalized(&namespace)).hiding("instanceId"));
    let ids = [
        namespace.urn("Building", "018-13")?,
        namespace.urn("City", "Paris")?,
        namespace.urn("Country", "France")?,
    ];
    delete_entities(&broker, &ids.iter().collect::<Vec<_>>()).await;
    Ok(())
}

/// Pins: Check that an entity can be retrieved with two levels linked entities in different join types and
/// representations
/// Clause: 5.7.1, 4.5.23
/// Expected: linked-entity retrieval answers carry objectType (Table 5.2.6-1)
#[gtest]
#[tokio::test]
async fn e018_13_02_flat_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = create_linked(&broker, &vocabulary, &namespace).await?;
    let client = broker.with_context(vocabulary.context())?;
    let received = client
        .with_responses()
        .entities()
        .retrieve_flattened(&building.id)
        .join_level(NonZeroU32::try_from(2)?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(flat_normalized(&namespace)).hiding("instanceId"));
    let ids = [
        namespace.urn("Building", "018-13")?,
        namespace.urn("City", "Paris")?,
        namespace.urn("Country", "France")?,
    ];
    delete_entities(&broker, &ids.iter().collect::<Vec<_>>()).await;
    Ok(())
}

/// Pins: Check that an entity can be retrieved with two levels linked entities in different join types and
/// representations
/// Clause: 5.7.1, 4.5.23
#[gtest]
#[tokio::test]
async fn e018_13_03_inline_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = create_linked(&broker, &vocabulary, &namespace).await?;
    let client = broker.with_context(vocabulary.context())?;
    let received = client
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&building.id)
        .join(InlineJoin::Inline)
        .join_level(NonZeroU32::try_from(2)?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(inline_simplified(&namespace)).hiding("instanceId"));
    let ids = [
        namespace.urn("Building", "018-13")?,
        namespace.urn("City", "Paris")?,
        namespace.urn("Country", "France")?,
    ];
    delete_entities(&broker, &ids.iter().collect::<Vec<_>>()).await;
    Ok(())
}

/// Pins: Check that an entity can be retrieved with two levels linked entities in different join types and
/// representations
/// Clause: 5.7.1, 4.5.23
#[gtest]
#[tokio::test]
async fn e018_13_04_flat_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = create_linked(&broker, &vocabulary, &namespace).await?;
    let client = broker.with_context(vocabulary.context())?;
    let received = client
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_flattened_simplified(&building.id)
        .join_level(NonZeroU32::try_from(2)?)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(flat_simplified(&namespace)).hiding("instanceId"));
    let ids = [
        namespace.urn("Building", "018-13")?,
        namespace.urn("City", "Paris")?,
        namespace.urn("Country", "France")?,
    ];
    delete_entities(&broker, &ids.iter().collect::<Vec<_>>()).await;
    Ok(())
}
