//! ETSI `019_30`: a Building with a multivalued `locatedAt` relationship is queried, normalized or simplified, with its
//! linked Cities joined inline or flat (clauses 5.7.2 and 4.5.23).

use crate::{
    suite_keywords::{create_entity, delete_entities},
    suite_payloads::city,
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
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{Entity, Relationship},
    query::Join,
};
use serde_json::json;
use std::num::NonZeroU32;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 3] = ["Building", "locatedAt", "City"];

/// Registers the case's `@context` and creates the Building located at the Cities Paris and Lyon, then the two Cities.
async fn created(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &TERMS).await?;
    let located_at = Relationship::builder(vec![namespace.urn("City", "Paris")?, namespace.urn("City", "Lyon")?]);
    let building = Entity::builder()
        .id(namespace.urn("Building", "019-30")?)
        .entity_type("Building")
        .relationship("locatedAt", located_at)
        .build()?;
    create_entity(broker, &vocabulary, &building).await?;
    create_entity(broker, &vocabulary, &city(namespace, "Paris")?).await?;
    create_entity(broker, &vocabulary, &city(namespace, "Lyon")?).await?;
    Ok(vocabulary)
}

/// The teardown of the cases: deletes the Building and its two Cities.
async fn delete(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    delete_entities(
        broker,
        &[
            &namespace.urn("Building", "019-30")?,
            &namespace.urn("City", "Paris")?,
            &namespace.urn("City", "Lyon")?,
        ],
    )
    .await;
    Ok(())
}

/// Pins: Check that entities with a multivalued relationship can be queried
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_30_01_query_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .ids(vec![namespace.urn("Building", "019-30")?])
        .entity_type("Building")
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-30"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": [namespace.urn_text("City", "Paris"), namespace.urn_text("City", "Lyon")],
                },
            },
        ]))
        .hiding("instanceId")
    );
    delete(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that entities with a multivalued relationship can be queried in simplified representation
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_30_02_query_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_simplified()
        .ids(vec![namespace.urn("Building", "019-30")?])
        .entity_type("Building")
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-30"),
                "type": "Building",
                "locatedAt": [namespace.urn_text("City", "Paris"), namespace.urn_text("City", "Lyon")],
            },
        ]))
        .hiding("instanceId")
    );
    delete(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that entities with a multivalued relationship can be queried with inline linked entities
/// Clause: 5.7.2, 4.5.23
#[gtest]
#[tokio::test]
async fn e019_30_03_query_inline_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .ids(vec![namespace.urn("Building", "019-30")?])
        .entity_type("Building")
        .join(Join::Inline)
        .join_level(NonZeroU32::MIN)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-30"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": [namespace.urn_text("City", "Paris"), namespace.urn_text("City", "Lyon")],
                    "entity": [
                        {"id": namespace.urn_text("City", "Paris"), "type": "City"},
                        {"id": namespace.urn_text("City", "Lyon"), "type": "City"},
                    ],
                },
            },
        ]))
        .hiding("instanceId")
    );
    delete(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that entities with a multivalued relationship can be queried with flat linked entities
/// Clause: 5.7.2, 4.5.23
#[gtest]
#[tokio::test]
async fn e019_30_04_query_flat_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .ids(vec![namespace.urn("Building", "019-30")?])
        .entity_type("Building")
        .join(Join::Flat)
        .join_level(NonZeroU32::MIN)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-30"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": [namespace.urn_text("City", "Paris"), namespace.urn_text("City", "Lyon")],
                },
            },
            {"id": namespace.urn_text("City", "Paris"), "type": "City"},
            {"id": namespace.urn_text("City", "Lyon"), "type": "City"},
        ]))
        .hiding("instanceId")
    );
    delete(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that entities with a multivalued relationship can be queried with inline linked entities in simplified
/// representation
/// Clause: 5.7.2, 4.5.23
#[gtest]
#[tokio::test]
async fn e019_30_05_query_inline_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_simplified()
        .ids(vec![namespace.urn("Building", "019-30")?])
        .entity_type("Building")
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .join(Join::Inline)
        .join_level(NonZeroU32::MIN)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-30"),
                "type": "Building",
                "locatedAt": [
                    {"id": namespace.urn_text("City", "Paris"), "type": "City"},
                    {"id": namespace.urn_text("City", "Lyon"), "type": "City"},
                ],
            },
        ]))
        .hiding("instanceId")
    );
    delete(&broker, &namespace).await?;
    Ok(())
}
