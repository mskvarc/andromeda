//! An entity whose Relationship targets an entity the broker does not hold is retrieved with that Relationship alone,
//! inline or flat (clauses 5.7.1 and 4.5.23).

use crate::entity_operations::create_entity;
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::InlineJoin,
    model::{Entity, EntityId, Input, Relationship},
};
use serde_json::{Value, json};
use std::num::NonZeroU32;

/// The terms of the case's `@context`.
const TERMS: [&str; 2] = ["Building", "locatedAt"];

/// The case's Building, whose `locatedAt` Relationship names `urn:ngsi-ld:City:<ns>-Paris` with no object type.
fn building_located_at_missing_city(namespace: &Namespace) -> Result<Entity<Input>> {
    let located_at = Relationship::builder(namespace.urn("City", "Paris")?);
    Ok(Entity::builder()
        .id(namespace.urn("Building", "018-14")?)
        .entity_type("Building")
        .relationship("locatedAt", located_at)
        .build()?)
}

/// The Building of the case as an inline join with no linked entity answers: its Relationship alone.
fn inline_expected(namespace: &Namespace) -> Value {
    json!({
        "id": namespace.urn_text("Building", "018-14"),
        "type": "Building",
        "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "Paris")},
    })
}

/// Pins: Check that an entity can be retrieved asking for a non-existent linked entity
/// Clause: 5.7.1, 4.5.23
#[gtest]
#[tokio::test]
async fn e018_14_01_inline() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = building_located_at_missing_city(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let client = broker.with_context(vocabulary.context())?;
    let received = client
        .with_responses()
        .entities()
        .retrieve(&building.id)
        .join(InlineJoin::Inline)
        .join_level(NonZeroU32::MIN)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(inline_expected(&namespace)).hiding("instanceId"));
    delete(&broker, &building.id).await;
    Ok(())
}

/// Pins: Check that an entity can be retrieved asking for a non-existent linked entity
/// Clause: 5.7.1, 4.5.23
/// Expected: the flat linked representation is an array (clause 4.5.23.3)
#[gtest]
#[tokio::test]
async fn e018_14_02_flat() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let building = building_located_at_missing_city(&namespace)?;
    create_entity(&broker, &vocabulary, &building).await?;
    let client = broker.with_context(vocabulary.context())?;
    let received = client
        .with_responses()
        .entities()
        .retrieve_flattened(&building.id)
        .join_level(NonZeroU32::MIN)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(&body, matches_document(json!([inline_expected(&namespace)])).hiding("instanceId"));
    delete(&broker, &building.id).await;
    Ok(())
}

/// Deletes `id` from the entity and temporal stores, as the cases' teardown does.
async fn delete(broker: &BrokerClient, id: &EntityId) {
    broker.entities().delete(id).send().await.any_answer();
    broker.temporal().delete(id).send().await.any_answer();
}
