//! ETSI `019_21`: Buildings linked to Cities are queried with an `omit` on the linking entity or on the linked entity,
//! joined inline or flat (clauses 5.7.2, 4.5.23 and 4.21).

use crate::{
    suite_keywords::{create_street_linked_pairs, delete_linked_pairs},
    suite_payloads::STREET_LINKED_TERMS,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    query::{Join, Locality, projection},
};
use serde_json::json;
use std::num::NonZeroU32;

/// The pair suffix of the Buildings and Cities of the cases.
const PAIR: &str = "019-21";

/// Pins: Check that entities can be queried with inline linked entity retrieval and omit on name
/// Clause: 5.7.2, 4.5.23, 4.21
/// Corrected: `fix(019_21)`: remove the omitted name from the Linked Cities too (5.7.2.5, 5.7.1.5)
#[gtest]
#[tokio::test]
async fn e019_21_01_query_inline_with_omit_on_linking_entity() -> Result<()> {
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
        .omit(projection::attributes(["name"]).build()?)
        .join(Join::Inline)
        .join_level(NonZeroU32::MIN)
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {
            "id": namespace.urn_text("Building", "019-21-1"),
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
            "locatedAt": {
                "type": "Relationship",
                "object": namespace.urn_text("City", "019-21-1"),
                "entity": {
                    "id": namespace.urn_text("City", "019-21-1"),
                    "type": "City",
                    "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
                    "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "France")},
                },
            },
        },
        {
            "id": namespace.urn_text("Building", "019-21-2"),
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
            "locatedAt": {
                "type": "Relationship",
                "object": namespace.urn_text("City", "019-21-2"),
                "entity": {
                    "id": namespace.urn_text("City", "019-21-2"),
                    "type": "City",
                    "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
                    "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "France")},
                },
            },
        },
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with flat linked entity retrieval and omit on name
/// Clause: 5.7.2, 4.5.23, 4.21
/// Corrected: `fix(019_21)`: remove the omitted name from the Linked Cities too (5.7.2.5, 5.7.1.5)
#[gtest]
#[tokio::test]
async fn e019_21_02_query_flat_with_omit_on_linking_entity() -> Result<()> {
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
        .omit(projection::attributes(["name"]).build()?)
        .join(Join::Flat)
        .join_level(NonZeroU32::MIN)
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {
            "id": namespace.urn_text("Building", "019-21-1"),
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-21-1")},
        },
        {
            "id": namespace.urn_text("Building", "019-21-2"),
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-21-2")},
        },
        {
            "id": namespace.urn_text("City", "019-21-1"),
            "type": "City",
            "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
            "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "France")},
        },
        {
            "id": namespace.urn_text("City", "019-21-2"),
            "type": "City",
            "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
            "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "France")},
        },
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with inline linked entity retrieval and omit on type
/// Clause: 5.7.2, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e019_21_03_query_inline_with_omit_on_linked_entity() -> Result<()> {
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
        .omit(projection::attributes(["name"]).linked("locatedAt", projection::attributes(["name"])).build()?)
        .join(Join::Inline)
        .join_level(NonZeroU32::MIN)
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {
            "id": namespace.urn_text("Building", "019-21-1"),
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
            "locatedAt": {
                "type": "Relationship",
                "object": namespace.urn_text("City", "019-21-1"),
                "entity": {
                    "id": namespace.urn_text("City", "019-21-1"),
                    "type": "City",
                    "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
                    "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "France")},
                },
            },
        },
        {
            "id": namespace.urn_text("Building", "019-21-2"),
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
            "locatedAt": {
                "type": "Relationship",
                "object": namespace.urn_text("City", "019-21-2"),
                "entity": {
                    "id": namespace.urn_text("City", "019-21-2"),
                    "type": "City",
                    "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
                    "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "France")},
                },
            },
        },
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with flat linked entity retrieval and omit on type
/// Clause: 5.7.2, 4.5.23, 4.21
#[gtest]
#[tokio::test]
async fn e019_21_04_query_flat_with_omit_on_linked_entity() -> Result<()> {
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
        .omit(projection::attributes(["name"]).linked("locatedAt", projection::attributes(["name"])).build()?)
        .join(Join::Flat)
        .join_level(NonZeroU32::MIN)
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {
            "id": namespace.urn_text("Building", "019-21-1"),
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-21-1")},
        },
        {
            "id": namespace.urn_text("Building", "019-21-2"),
            "type": "Building",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-21-2")},
        },
        {
            "id": namespace.urn_text("City", "019-21-1"),
            "type": "City",
            "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
            "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "France")},
        },
        {
            "id": namespace.urn_text("City", "019-21-2"),
            "type": "City",
            "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
            "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "France")},
        },
    ]);
    expect_that!(&body, matches_document(expected).hiding("instanceId"));
    delete_linked_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}
