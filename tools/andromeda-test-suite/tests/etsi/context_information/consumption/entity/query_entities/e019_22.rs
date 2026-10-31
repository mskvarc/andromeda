//! Buildings linked to Cities linked to Countries are queried with a `pick` on the linked entities, joined inline or
//! flat, at one and at two levels (clauses 5.7.2, 4.5.23 and 4.21).

use crate::{
    linked_entity_fixtures::{create_building_city_country_pairs, delete_building_city_country_pairs},
    linked_place_payloads::PARIS_IN_FRANCE_TERMS,
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
    http::headers::Accept,
    query::{
        Join,
        Locality,
        projection::{self, EntityMember},
    },
};
use serde_json::json;
use std::num::NonZeroU32;

/// The pair suffix of the Buildings, Cities and Countries of the cases.
const PAIR: &str = "019-22";

/// Registers the case's `@context` and creates its two chains.
async fn created(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &PARIS_IN_FRANCE_TERMS).await?;
    create_building_city_country_pairs(broker, &vocabulary, namespace, PAIR).await?;
    Ok(vocabulary)
}

/// Pins: Check that entities can be queried with inline join and pick on linked entity
/// Clause: 5.7.2, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e019_22_01_query_inline_with_pick_on_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let pick = projection::member(EntityMember::Id)
        .linked("locatedAt", projection::member(EntityMember::Id).attribute("name"))
        .build()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building")
        .locality(Locality::Local)
        .join(Join::Inline)
        .join_level(NonZeroU32::try_from(2)?)
        .pick(pick)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-22-1"),
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-22-1"),
                    "entity": {
                        "id": namespace.urn_text("City", "019-22-1"),
                        "name": {"type": "Property", "value": "Paris"},
                    },
                },
            },
            {
                "id": namespace.urn_text("Building", "019-22-2"),
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-22-2"),
                    "entity": {
                        "id": namespace.urn_text("City", "019-22-2"),
                        "name": {"type": "Property", "value": "Paris"},
                    },
                },
            },
        ]))
        .hiding("instanceId")
    );
    delete_building_city_country_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with flat join and pick on linked entity
/// Clause: 5.7.2, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e019_22_02_query_flat_with_pick_on_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let pick = projection::member(EntityMember::Id)
        .linked("locatedAt", projection::member(EntityMember::Id).attribute("name"))
        .build()?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building")
        .locality(Locality::Local)
        .join(Join::Flat)
        .join_level(NonZeroU32::try_from(2)?)
        .pick(pick)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-22-1"),
                "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-22-1")},
            },
            {
                "id": namespace.urn_text("Building", "019-22-2"),
                "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-22-2")},
            },
            {
                "id": namespace.urn_text("City", "019-22-1"),
                "name": {"type": "Property", "value": "Paris"},
            },
            {
                "id": namespace.urn_text("City", "019-22-2"),
                "name": {"type": "Property", "value": "Paris"},
            },
        ]))
        .hiding("instanceId")
    );
    delete_building_city_country_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with inline join and pick on second level linked entity
/// Clause: 5.7.2, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e019_22_03_query_inline_with_pick_on_second_level_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
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
        .query_projected()
        .entity_type("Building")
        .locality(Locality::Local)
        .join(Join::Inline)
        .join_level(NonZeroU32::try_from(2)?)
        .pick(pick)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-22-1"),
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-22-1"),
                    "entity": {
                        "id": namespace.urn_text("City", "019-22-1"),
                        "name": {"type": "Property", "value": "Paris"},
                        "isInCountry": {
                            "type": "Relationship",
                            "object": namespace.urn_text("Country", "019-22-1"),
                            "entity": {
                                "id": namespace.urn_text("Country", "019-22-1"),
                                "description": {"type": "Property", "value": "France is an European country"},
                            },
                        },
                    },
                },
            },
            {
                "id": namespace.urn_text("Building", "019-22-2"),
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-22-2"),
                    "entity": {
                        "id": namespace.urn_text("City", "019-22-2"),
                        "name": {"type": "Property", "value": "Paris"},
                        "isInCountry": {
                            "type": "Relationship",
                            "object": namespace.urn_text("Country", "019-22-2"),
                            "entity": {
                                "id": namespace.urn_text("Country", "019-22-2"),
                                "description": {"type": "Property", "value": "France is an European country"},
                            },
                        },
                    },
                },
            },
        ]))
        .hiding("instanceId")
    );
    delete_building_city_country_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with flat join and pick on second level linked entity
/// Clause: 5.7.2, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e019_22_04_query_flat_with_pick_on_second_level_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
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
        .query_projected()
        .entity_type("Building")
        .locality(Locality::Local)
        .join(Join::Flat)
        .join_level(NonZeroU32::try_from(2)?)
        .pick(pick)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-22-1"),
                "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-22-1")},
            },
            {
                "id": namespace.urn_text("Building", "019-22-2"),
                "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-22-2")},
            },
            {
                "id": namespace.urn_text("City", "019-22-1"),
                "name": {"type": "Property", "value": "Paris"},
                "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "019-22-1")},
            },
            {
                "id": namespace.urn_text("City", "019-22-2"),
                "name": {"type": "Property", "value": "Paris"},
                "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "019-22-2")},
            },
            {
                "id": namespace.urn_text("Country", "019-22-1"),
                "description": {"type": "Property", "value": "France is an European country"},
            },
            {
                "id": namespace.urn_text("Country", "019-22-2"),
                "description": {"type": "Property", "value": "France is an European country"},
            },
        ]))
        .hiding("instanceId")
    );
    delete_building_city_country_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}
