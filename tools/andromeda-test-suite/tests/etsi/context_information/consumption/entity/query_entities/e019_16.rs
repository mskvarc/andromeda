//! ETSI `019_16`: Buildings, Cities and Countries linked over two levels are queried with the linked entities joined
//! inline or flat, in the normalized and the simplified representation (clauses 5.7.2 and 4.5.23).

use crate::{
    suite_keywords::{create_entity, delete_entities},
    suite_payloads::{building_located_at_city, city_located_in_country, country},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    problem::answered,
    teardown::AnyAnswer,
    vocabulary::Vocabulary,
    wire::{json_body, send_to},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::Accept,
        operation::{CreateEntity, RepresentationSpelling},
    },
    model::{Entity, Input},
    query::Join,
};
use serde_json::json;
use std::num::NonZeroU32;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 4] = ["Building", "locatedAt", "City", "Country"];

/// Creates the two chains of the cases, Building `<ns>-019-16-1` located at City `019-16-1` located at Country
/// `019-16-1`, and the same for suffix `019-16-2`, in `vocabulary`'s context. The second Country is created with its
/// answer unchecked.
async fn create_chains(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    create_entity(broker, vocabulary, &building_located_at_city(namespace, "019-16-1")?).await?;
    create_entity(broker, vocabulary, &city_located_in_country(namespace, "019-16-1")?).await?;
    create_entity(broker, vocabulary, &country(namespace, "019-16-1")?).await?;
    create_entity(broker, vocabulary, &building_located_at_city(namespace, "019-16-2")?).await?;
    create_entity(broker, vocabulary, &city_located_in_country(namespace, "019-16-2")?).await?;
    create_unchecked(broker, vocabulary, &country(namespace, "019-16-2")?).await
}

/// Posts `entity` with the vocabulary as a `@context` list in the body, as `create_entity` does, without checking the
/// answer.
async fn create_unchecked(broker: &BrokerClient, vocabulary: &Vocabulary, entity: &Entity<Input>) -> Result<()> {
    let client = broker
        .with_context(vocabulary.context_list())?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    send_to(&client, &CreateEntity::builder().entity(entity).build(), "entities/")
        .await
        .any_answer();
    Ok(())
}

/// Deletes the entities of the chains.
async fn delete_chains(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    delete_entities(
        broker,
        &[
            &namespace.urn("Building", "019-16-1")?,
            &namespace.urn("City", "019-16-1")?,
            &namespace.urn("Country", "019-16-1")?,
            &namespace.urn("Building", "019-16-2")?,
            &namespace.urn("City", "019-16-2")?,
            &namespace.urn("Country", "019-16-2")?,
        ],
    )
    .await;
    Ok(())
}

/// Pins: Check that entities can be queried with two levels linked entities in different join types and representations
/// Clause: 5.7.2, 4.5.23
/// Corrected: fix(linked-objecttype): linked-entity retrieval expectations carry objecttype (table 5.2.6-1)
#[gtest]
#[tokio::test]
async fn e019_16_01_query_inline_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_chains(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .join(Join::Inline)
        .join_level(NonZeroU32::try_from(2)?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-16-1"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-16-1"),
                    "objectType": "City",
                    "entity": {
                        "id": namespace.urn_text("City", "019-16-1"),
                        "type": "City",
                        "locatedAt": {
                            "type": "Relationship",
                            "object": namespace.urn_text("Country", "019-16-1"),
                            "objectType": "Country",
                            "entity": {"id": namespace.urn_text("Country", "019-16-1"), "type": "Country"},
                        },
                    },
                },
            },
            {
                "id": namespace.urn_text("Building", "019-16-2"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-16-2"),
                    "objectType": "City",
                    "entity": {
                        "id": namespace.urn_text("City", "019-16-2"),
                        "type": "City",
                        "locatedAt": {
                            "type": "Relationship",
                            "object": namespace.urn_text("Country", "019-16-2"),
                            "objectType": "Country",
                            "entity": {"id": namespace.urn_text("Country", "019-16-2"), "type": "Country"},
                        },
                    },
                },
            },
        ]))
        .hiding("instanceId")
    );
    delete_chains(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with two levels linked entities in different join types and representations
/// Clause: 5.7.2, 4.5.23
/// Corrected: fix(linked-objecttype): linked-entity retrieval expectations carry objecttype (table 5.2.6-1)
#[gtest]
#[tokio::test]
async fn e019_16_02_query_flat_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_chains(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .join(Join::Flat)
        .join_level(NonZeroU32::try_from(2)?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-16-1"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-16-1"),
                    "objectType": "City",
                },
            },
            {
                "id": namespace.urn_text("Building", "019-16-2"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-16-2"),
                    "objectType": "City",
                },
            },
            {
                "id": namespace.urn_text("City", "019-16-1"),
                "type": "City",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("Country", "019-16-1"),
                    "objectType": "Country",
                },
            },
            {
                "id": namespace.urn_text("City", "019-16-2"),
                "type": "City",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("Country", "019-16-2"),
                    "objectType": "Country",
                },
            },
            {"id": namespace.urn_text("Country", "019-16-1"), "type": "Country"},
            {"id": namespace.urn_text("Country", "019-16-2"), "type": "Country"},
        ]))
        .hiding("instanceId")
    );
    delete_chains(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with two levels linked entities in different join types and representations
/// Clause: 5.7.2, 4.5.23
#[gtest]
#[tokio::test]
async fn e019_16_03_query_inline_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_chains(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_simplified()
        .entity_type("Building")
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .join(Join::Inline)
        .join_level(NonZeroU32::try_from(2)?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-16-1"),
                "type": "Building",
                "locatedAt": {
                    "id": namespace.urn_text("City", "019-16-1"),
                    "type": "City",
                    "locatedAt": {"id": namespace.urn_text("Country", "019-16-1"), "type": "Country"},
                },
            },
            {
                "id": namespace.urn_text("Building", "019-16-2"),
                "type": "Building",
                "locatedAt": {
                    "id": namespace.urn_text("City", "019-16-2"),
                    "type": "City",
                    "locatedAt": {"id": namespace.urn_text("Country", "019-16-2"), "type": "Country"},
                },
            },
        ]))
        .hiding("instanceId")
    );
    delete_chains(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with two levels linked entities in different join types and representations
/// Clause: 5.7.2, 4.5.23
#[gtest]
#[tokio::test]
async fn e019_16_04_query_flat_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_chains(&broker, &vocabulary, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_simplified()
        .entity_type("Building")
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .join(Join::Flat)
        .join_level(NonZeroU32::try_from(2)?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-16-1"),
                "type": "Building",
                "locatedAt": namespace.urn_text("City", "019-16-1"),
            },
            {
                "id": namespace.urn_text("Building", "019-16-2"),
                "type": "Building",
                "locatedAt": namespace.urn_text("City", "019-16-2"),
            },
            {
                "id": namespace.urn_text("City", "019-16-1"),
                "type": "City",
                "locatedAt": namespace.urn_text("Country", "019-16-1"),
            },
            {
                "id": namespace.urn_text("City", "019-16-2"),
                "type": "City",
                "locatedAt": namespace.urn_text("Country", "019-16-2"),
            },
            {"id": namespace.urn_text("Country", "019-16-1"), "type": "Country"},
            {"id": namespace.urn_text("Country", "019-16-2"), "type": "Country"},
        ]))
        .hiding("instanceId")
    );
    delete_chains(&broker, &namespace).await?;
    Ok(())
}
