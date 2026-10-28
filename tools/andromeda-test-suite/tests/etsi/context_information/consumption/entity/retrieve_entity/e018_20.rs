//! ETSI `018_20`: an entity is retrieved with a `pick` on a linked entity, and on a second level linked entity, joined
//! inline or flat (clauses 5.7.1, 4.5.23 and 4.21).

use crate::{
    suite_keywords::{create_paris_in_france, delete_entities},
    suite_payloads::PARIS_IN_FRANCE_TERMS,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::InlineJoin},
    query::{
        Locality,
        projection::{self, EntityMember},
    },
};
use serde_json::json;
use std::num::NonZeroU32;

/// Pins: Check that an entity can be retrieved with inline join and pick on linked entity
/// Clause: 5.7.1, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e018_20_01_retrieve_inline_with_pick_on_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &PARIS_IN_FRANCE_TERMS).await?;
    let [building, city, country] = create_paris_in_france(&broker, &vocabulary, &namespace, "018-20").await?;
    let pick = projection::member(EntityMember::Id)
        .linked("locatedAt", projection::member(EntityMember::Id).attribute("name"))
        .build()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve_projected(&building)
        .join(InlineJoin::Inline)
        .join_level(NonZeroU32::try_from(2)?)
        .locality(Locality::Local)
        .pick(pick)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.to_string(),
        "locatedAt": {
            "type": "Relationship",
            "object": city.to_string(),
            "entity": {"id": city.to_string(), "name": {"type": "Property", "value": "Paris"}},
        },
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&building, &city, &country]).await;
    Ok(())
}

/// Pins: Check that an entity can be retrieved with flat join and pick on linked entity
/// Clause: 5.7.1, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e018_20_02_retrieve_flat_with_pick_on_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &PARIS_IN_FRANCE_TERMS).await?;
    let [building, city, country] = create_paris_in_france(&broker, &vocabulary, &namespace, "018-20").await?;
    let pick = projection::member(EntityMember::Id)
        .linked("locatedAt", projection::member(EntityMember::Id).attribute("name"))
        .build()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve_flattened_projected(&building)
        .join_level(NonZeroU32::try_from(2)?)
        .locality(Locality::Local)
        .pick(pick)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {
            "id": building.to_string(),
            "locatedAt": {"type": "Relationship", "object": city.to_string()},
        },
        {"id": city.to_string(), "name": {"type": "Property", "value": "Paris"}},
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&building, &city, &country]).await;
    Ok(())
}

/// Pins: Check that an entity can be retrieved with inline join and pick on 2nd level linked entity
/// Clause: 5.7.1, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e018_20_03_retrieve_inline_with_pick_on_second_level_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &PARIS_IN_FRANCE_TERMS).await?;
    let [building, city, country] = create_paris_in_france(&broker, &vocabulary, &namespace, "018-20").await?;
    let pick = projection::member(EntityMember::Id)
        .linked(
            "locatedAt",
            projection::member(EntityMember::Id)
                .attribute("name")
                .linked("isInCountry", projection::member(EntityMember::Id).attribute("description")),
        )
        .build()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve_projected(&building)
        .join(InlineJoin::Inline)
        .join_level(NonZeroU32::try_from(2)?)
        .locality(Locality::Local)
        .pick(pick)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!({
        "id": building.to_string(),
        "locatedAt": {
            "type": "Relationship",
            "object": city.to_string(),
            "entity": {
                "id": city.to_string(),
                "name": {"type": "Property", "value": "Paris"},
                "isInCountry": {
                    "type": "Relationship",
                    "object": country.to_string(),
                    "entity": {
                        "id": country.to_string(),
                        "description": {"type": "Property", "value": "France is an European country"},
                    },
                },
            },
        },
    });
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&building, &city, &country]).await;
    Ok(())
}

/// Pins: Check that an entity can be retrieved with flat join and pick on 2nd level linked entity
/// Clause: 5.7.1, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e018_20_04_retrieve_flat_with_pick_on_second_level_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &PARIS_IN_FRANCE_TERMS).await?;
    let [building, city, country] = create_paris_in_france(&broker, &vocabulary, &namespace, "018-20").await?;
    let pick = projection::member(EntityMember::Id)
        .linked(
            "locatedAt",
            projection::member(EntityMember::Id)
                .attribute("name")
                .linked("isInCountry", projection::member(EntityMember::Id).attribute("description")),
        )
        .build()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .retrieve_flattened_projected(&building)
        .join_level(NonZeroU32::try_from(2)?)
        .locality(Locality::Local)
        .pick(pick)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {
            "id": building.to_string(),
            "locatedAt": {"type": "Relationship", "object": city.to_string()},
        },
        {
            "id": city.to_string(),
            "name": {"type": "Property", "value": "Paris"},
            "isInCountry": {"type": "Relationship", "object": country.to_string()},
        },
        {
            "id": country.to_string(),
            "description": {"type": "Property", "value": "France is an European country"},
        },
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_entities(&broker, &[&building, &city, &country]).await;
    Ok(())
}
