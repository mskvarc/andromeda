//! ETSI `019_13`: Buildings linked to Cities are queried with the linked entities joined inline or flat, in the
//! normalized and the simplified representation (clauses 5.7.2 and 4.5.23).

use crate::suite_keywords::{create_linked_pairs, delete_linked_pairs};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    query::Join,
};
use serde_json::json;
use std::num::NonZeroU32;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 3] = ["Building", "locatedAt", "City"];

/// The pair suffix of the Buildings and Cities of the cases.
const PAIR: &str = "019-13";

/// Pins: Check that entities can be queried with a linked entity in different join types and representations
/// Clause: 5.7.2, 4.5.23
/// Corrected: `fix(019_13)`: normalized linked expectations carry objecttype (table 5.2.6-1)
#[gtest]
#[tokio::test]
async fn e019_13_01_query_inline_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_linked_pairs(&broker, &vocabulary, &namespace, PAIR).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
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
                "id": namespace.urn_text("Building", "019-13-1"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-13-1"),
                    "objectType": "City",
                    "entity": {"id": namespace.urn_text("City", "019-13-1"), "type": "City"},
                },
            },
            {
                "id": namespace.urn_text("Building", "019-13-2"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-13-2"),
                    "objectType": "City",
                    "entity": {"id": namespace.urn_text("City", "019-13-2"), "type": "City"},
                },
            },
        ]))
        .hiding("instanceId")
    );
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with a linked entity in different join types and representations
/// Clause: 5.7.2, 4.5.23
/// Corrected: `fix(019_13)`: normalized linked expectations carry objecttype (table 5.2.6-1)
#[gtest]
#[tokio::test]
async fn e019_13_02_query_flat_normalized() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_linked_pairs(&broker, &vocabulary, &namespace, PAIR).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
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
                "id": namespace.urn_text("Building", "019-13-1"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-13-1"),
                    "objectType": "City",
                },
            },
            {
                "id": namespace.urn_text("Building", "019-13-2"),
                "type": "Building",
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-13-2"),
                    "objectType": "City",
                },
            },
            {"id": namespace.urn_text("City", "019-13-1"), "type": "City"},
            {"id": namespace.urn_text("City", "019-13-2"), "type": "City"},
        ]))
        .hiding("instanceId")
    );
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with a linked entity in different join types and representations
/// Clause: 5.7.2, 4.5.23
#[gtest]
#[tokio::test]
async fn e019_13_03_query_inline_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_linked_pairs(&broker, &vocabulary, &namespace, PAIR).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_simplified()
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
                "id": namespace.urn_text("Building", "019-13-1"),
                "type": "Building",
                "locatedAt": {"id": namespace.urn_text("City", "019-13-1"), "type": "City"},
            },
            {
                "id": namespace.urn_text("Building", "019-13-2"),
                "type": "Building",
                "locatedAt": {"id": namespace.urn_text("City", "019-13-2"), "type": "City"},
            },
        ]))
        .hiding("instanceId")
    );
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with a linked entity in different join types and representations
/// Clause: 5.7.2, 4.5.23
#[gtest]
#[tokio::test]
async fn e019_13_04_query_flat_simplified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    create_linked_pairs(&broker, &vocabulary, &namespace, PAIR).await?;
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query_simplified()
        .entity_type("Building")
        .spelling(RepresentationSpelling::default().in_options().key_values())
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
                "id": namespace.urn_text("Building", "019-13-1"),
                "type": "Building",
                "locatedAt": namespace.urn_text("City", "019-13-1"),
            },
            {
                "id": namespace.urn_text("Building", "019-13-2"),
                "type": "Building",
                "locatedAt": namespace.urn_text("City", "019-13-2"),
            },
            {"id": namespace.urn_text("City", "019-13-1"), "type": "City"},
            {"id": namespace.urn_text("City", "019-13-2"), "type": "City"},
        ]))
        .hiding("instanceId")
    );
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}
