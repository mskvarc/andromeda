//! Buildings linked to Cities are queried with a `pick` on the linking entity or on the linked entity, joined inline or
//! flat (clauses 5.7.2, 4.5.23 and 4.21).

use crate::{
    linked_entity_fixtures::{create_street_linked_pairs, delete_linked_pairs},
    linked_place_payloads::STREET_LINKED_TERMS,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
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

/// The pair suffix of the Buildings and Cities of the cases.
const PAIR: &str = "019-20";

/// Pins: Check that entities can be queried with inline join and pick on linking entity
/// Clause: 5.7.2, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e019_20_01_query_inline_with_pick_on_linking_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &STREET_LINKED_TERMS).await?;
    create_street_linked_pairs(&broker, &vocabulary, &namespace, PAIR).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building")
        .pick(projection::member(EntityMember::Id).attribute("name").build()?)
        .join(Join::Inline)
        .join_level(NonZeroU32::MIN)
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {"id": namespace.urn_text("Building", "019-20-1"), "name": {"type": "Property", "value": "Eiffel Tower"}},
        {"id": namespace.urn_text("Building", "019-20-2"), "name": {"type": "Property", "value": "Eiffel Tower"}},
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with flat join and pick on linking entity
/// Clause: 5.7.2, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e019_20_02_query_flat_with_pick_on_linking_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &STREET_LINKED_TERMS).await?;
    create_street_linked_pairs(&broker, &vocabulary, &namespace, PAIR).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building")
        .pick(projection::member(EntityMember::Id).attribute("name").build()?)
        .join(Join::Flat)
        .join_level(NonZeroU32::MIN)
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {"id": namespace.urn_text("Building", "019-20-1"), "name": {"type": "Property", "value": "Eiffel Tower"}},
        {"id": namespace.urn_text("Building", "019-20-2"), "name": {"type": "Property", "value": "Eiffel Tower"}},
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with inline join and pick on linked entity
/// Clause: 5.7.2, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e019_20_03_query_inline_with_pick_on_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &STREET_LINKED_TERMS).await?;
    create_street_linked_pairs(&broker, &vocabulary, &namespace, PAIR).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building")
        .pick(
            projection::member(EntityMember::Id)
                .linked("locatedAt", projection::member(EntityMember::Id).attribute("name"))
                .build()?,
        )
        .join(Join::Inline)
        .join_level(NonZeroU32::MIN)
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {
            "id": namespace.urn_text("Building", "019-20-1"),
            "locatedAt": {
                "type": "Relationship",
                "object": namespace.urn_text("City", "019-20-1"),
                "entity": {"id": namespace.urn_text("City", "019-20-1"), "name": {"type": "Property", "value": "Paris"}},
            },
        },
        {
            "id": namespace.urn_text("Building", "019-20-2"),
            "locatedAt": {
                "type": "Relationship",
                "object": namespace.urn_text("City", "019-20-2"),
                "entity": {"id": namespace.urn_text("City", "019-20-2"), "name": {"type": "Property", "value": "Paris"}},
            },
        },
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with flat join and pick on linked entity
/// Clause: 5.7.2, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e019_20_04_query_flat_with_pick_on_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &STREET_LINKED_TERMS).await?;
    create_street_linked_pairs(&broker, &vocabulary, &namespace, PAIR).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_projected()
        .entity_type("Building")
        .pick(
            projection::member(EntityMember::Id)
                .linked("locatedAt", projection::member(EntityMember::Id).attribute("name"))
                .build()?,
        )
        .join(Join::Flat)
        .join_level(NonZeroU32::MIN)
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {
            "id": namespace.urn_text("Building", "019-20-1"),
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-20-1")},
        },
        {
            "id": namespace.urn_text("Building", "019-20-2"),
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-20-2")},
        },
        {"id": namespace.urn_text("City", "019-20-1"), "name": {"type": "Property", "value": "Paris"}},
        {"id": namespace.urn_text("City", "019-20-2"), "name": {"type": "Property", "value": "Paris"}},
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}
