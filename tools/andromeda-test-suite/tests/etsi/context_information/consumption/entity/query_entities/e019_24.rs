//! ETSI `019_24`: Buildings linked to Cities are queried in the normalized and the simplified representation, with the
//! linked entities joined inline or flat, through the `entityOperations/query` binding (clauses 5.7.2, 4.5.23 and
//! 6.23).

use crate::{
    suite_keywords::{create_entity, delete_entities},
    suite_payloads::{building_located_in_city, city},
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
    http::operation::{QueryMethod, RepresentationSpelling},
    model::NgsiLdMediaType,
    query::Join,
};
use serde_json::json;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 3] = ["Building", "locatedAt", "City"];

/// The pair suffix of the Buildings and Cities of the cases.
const PAIR: &str = "019-24";

/// Registers the case's `@context` and creates two Buildings, each located at its City, with the Cities.
async fn created(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &TERMS).await?;
    for suffix in [format!("{PAIR}-1"), format!("{PAIR}-2")] {
        create_entity(broker, &vocabulary, &building_located_in_city(namespace, &suffix, &suffix)?).await?;
        create_entity(broker, &vocabulary, &city(namespace, &suffix)?).await?;
    }
    Ok(vocabulary)
}

/// The teardown of the cases: deletes the Buildings and Cities that [`created`] creates.
async fn delete(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    delete_entities(
        broker,
        &[
            &namespace.urn("Building", format!("{PAIR}-1"))?,
            &namespace.urn("City", format!("{PAIR}-1"))?,
            &namespace.urn("Building", format!("{PAIR}-2"))?,
            &namespace.urn("City", format!("{PAIR}-2"))?,
        ],
    )
    .await;
    Ok(())
}

/// Pins: Check that entities can be queried with a linked entity in different join types and representations
/// Clause: 5.7.2, 4.5.23, 6.23
#[gtest]
#[tokio::test]
async fn e019_24_01_query_inline_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .method(QueryMethod::Post)
        .join(Join::Inline)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-24-1"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-24-1"),
                    "entity": {"id": namespace.urn_text("City", "019-24-1"), "type": "City"},
                },
            },
            {
                "id": namespace.urn_text("Building", "019-24-2"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-24-2"),
                    "entity": {"id": namespace.urn_text("City", "019-24-2"), "type": "City"},
                },
            },
        ]))
        .hiding("instanceId")
    );
    delete(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with a linked entity in different join types and representations
/// Clause: 5.7.2, 4.5.23, 6.23
#[gtest]
#[tokio::test]
async fn e019_24_02_query_flat_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .query()
        .entity_type("Building")
        .method(QueryMethod::Post)
        .join(Join::Flat)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-24-1"),
                "type": "Building",
                "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-24-1")},
            },
            {
                "id": namespace.urn_text("Building", "019-24-2"),
                "type": "Building",
                "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-24-2")},
            },
            {"id": namespace.urn_text("City", "019-24-1"), "type": "City"},
            {"id": namespace.urn_text("City", "019-24-2"), "type": "City"},
        ]))
        .hiding("instanceId")
    );
    delete(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with a linked entity in different join types and representations
/// Clause: 5.7.2, 4.5.23, 6.23
#[gtest]
#[tokio::test]
async fn e019_24_03_query_inline_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .query_simplified()
        .entity_type("Building")
        .method(QueryMethod::Post)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .join(Join::Inline)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-24-1"),
                "type": "Building",
                "locatedAt": {"id": namespace.urn_text("City", "019-24-1"), "type": "City"},
            },
            {
                "id": namespace.urn_text("Building", "019-24-2"),
                "type": "Building",
                "locatedAt": {"id": namespace.urn_text("City", "019-24-2"), "type": "City"},
            },
        ]))
        .hiding("instanceId")
    );
    delete(&broker, &namespace).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with a linked entity in different join types and representations
/// Clause: 5.7.2, 4.5.23, 6.23
#[gtest]
#[tokio::test]
async fn e019_24_04_query_flat_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::Json)
        .with_responses()
        .entities()
        .query_simplified()
        .entity_type("Building")
        .method(QueryMethod::Post)
        .spelling(RepresentationSpelling::default().in_options().key_values())
        .join(Join::Flat)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-24-1"),
                "type": "Building",
                "locatedAt": namespace.urn_text("City", "019-24-1"),
            },
            {
                "id": namespace.urn_text("Building", "019-24-2"),
                "type": "Building",
                "locatedAt": namespace.urn_text("City", "019-24-2"),
            },
            {"id": namespace.urn_text("City", "019-24-1"), "type": "City"},
            {"id": namespace.urn_text("City", "019-24-2"), "type": "City"},
        ]))
        .hiding("instanceId")
    );
    delete(&broker, &namespace).await?;
    Ok(())
}
