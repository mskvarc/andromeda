//! ETSI `019_23`: Buildings linked to Cities linked to Countries are queried with an `omit` on the linked entities,
//! joined inline or flat, at one and at two levels (clauses 5.7.2, 4.5.23 and 4.21).

use crate::{
    suite_keywords::{create_building_city_country_pairs, delete_building_city_country_pairs},
    suite_payloads::PARIS_IN_FRANCE_TERMS,
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
const PAIR: &str = "019-23";

/// Registers the case's `@context` and creates its two chains.
async fn created(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &PARIS_IN_FRANCE_TERMS).await?;
    create_building_city_country_pairs(broker, &vocabulary, namespace, PAIR).await?;
    Ok(vocabulary)
}

/// Pins: Check that entities can be queried with inline join and omit on linked entity
/// Clause: 5.7.2, 4.5.23, 4.21
/// Changed: the second Country is created with a 201 check, where the Hurl file accepts any answer to that creation
/// Defect: D-31: Andromeda carries the Linked Entity Term locatedAt{name} past the City to the Country, which no term
/// addresses, so the Country comes back without name where the expectation keeps it
#[gtest]
#[tokio::test]
async fn e019_23_01_query_inline_with_omit_on_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let omit = projection::member(EntityMember::Type)
        .linked("locatedAt", projection::attributes(["name"]))
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
        .omit(omit)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-23-1"),
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-23-1"),
                    "entity": {
                        "id": namespace.urn_text("City", "019-23-1"),
                        "type": "City",
                        "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
                        "isInCountry": {
                            "type": "Relationship",
                            "object": namespace.urn_text("Country", "019-23-1"),
                            "entity": {
                                "id": namespace.urn_text("Country", "019-23-1"),
                                "type": "Country",
                                "description": {"type": "Property", "value": "France is an European country"},
                                "name": {"type": "Property", "value": "France"},
                            },
                        },
                    },
                },
            },
            {
                "id": namespace.urn_text("Building", "019-23-2"),
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-23-2"),
                    "entity": {
                        "id": namespace.urn_text("City", "019-23-2"),
                        "type": "City",
                        "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
                        "isInCountry": {
                            "type": "Relationship",
                            "object": namespace.urn_text("Country", "019-23-2"),
                            "entity": {
                                "id": namespace.urn_text("Country", "019-23-2"),
                                "type": "Country",
                                "description": {"type": "Property", "value": "France is an European country"},
                                "name": {"type": "Property", "value": "France"},
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

/// Pins: Check that entities can be queried with flat join and omit on linked entity
/// Clause: 5.7.2, 4.5.23, 4.21
/// Changed: the second Country is created with a 201 check, where the Hurl file accepts any answer to that creation
/// Defect: D-31: Andromeda carries the Linked Entity Term locatedAt{name} past the City to the Country, which no term
/// addresses, so the Country comes back without name where the expectation keeps it
#[gtest]
#[tokio::test]
async fn e019_23_02_query_flat_with_omit_on_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let omit = projection::member(EntityMember::Type)
        .linked("locatedAt", projection::attributes(["name"]))
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
        .omit(omit)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-23-1"),
                "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-23-1")},
            },
            {
                "id": namespace.urn_text("Building", "019-23-2"),
                "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-23-2")},
            },
            {
                "id": namespace.urn_text("City", "019-23-1"),
                "type": "City",
                "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
                "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "019-23-1")},
            },
            {
                "id": namespace.urn_text("City", "019-23-2"),
                "type": "City",
                "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
                "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "019-23-2")},
            },
            {
                "id": namespace.urn_text("Country", "019-23-1"),
                "type": "Country",
                "description": {"type": "Property", "value": "France is an European country"},
                "name": {"type": "Property", "value": "France"},
            },
            {
                "id": namespace.urn_text("Country", "019-23-2"),
                "type": "Country",
                "description": {"type": "Property", "value": "France is an European country"},
                "name": {"type": "Property", "value": "France"},
            },
        ]))
        .hiding("instanceId")
    );
    delete_building_city_country_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}

/// Pins: Check that entities can be queried with inline join and omit on second level linked entity
/// Clause: 5.7.2, 4.5.23, 4.21
/// Changed: the second Country is created with a 201 check, where the Hurl file accepts any answer to that creation
#[gtest]
#[tokio::test]
async fn e019_23_03_query_inline_with_omit_on_second_level_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let omit = projection::member(EntityMember::Type)
        .linked(
            "locatedAt",
            projection::attributes(["name"]).linked("isInCountry", projection::attributes(["description"])),
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
        .omit(omit)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-23-1"),
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-23-1"),
                    "entity": {
                        "id": namespace.urn_text("City", "019-23-1"),
                        "type": "City",
                        "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
                        "isInCountry": {
                            "type": "Relationship",
                            "object": namespace.urn_text("Country", "019-23-1"),
                            "entity": {
                                "id": namespace.urn_text("Country", "019-23-1"),
                                "type": "Country",
                                "name": {"type": "Property", "value": "France"},
                            },
                        },
                    },
                },
            },
            {
                "id": namespace.urn_text("Building", "019-23-2"),
                "locatedAt": {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "019-23-2"),
                    "entity": {
                        "id": namespace.urn_text("City", "019-23-2"),
                        "type": "City",
                        "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
                        "isInCountry": {
                            "type": "Relationship",
                            "object": namespace.urn_text("Country", "019-23-2"),
                            "entity": {
                                "id": namespace.urn_text("Country", "019-23-2"),
                                "type": "Country",
                                "name": {"type": "Property", "value": "France"},
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

/// Pins: Check that entities can be queried with flat join and omit on second level linked entity
/// Clause: 5.7.2, 4.5.23, 4.21
/// Changed: the second Country is created with a 201 check, where the Hurl file accepts any answer to that creation
#[gtest]
#[tokio::test]
async fn e019_23_04_query_flat_with_omit_on_second_level_linked_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = created(&broker, &namespace).await?;
    let omit = projection::member(EntityMember::Type)
        .linked(
            "locatedAt",
            projection::attributes(["name"]).linked("isInCountry", projection::attributes(["description"])),
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
        .omit(omit)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([
            {
                "id": namespace.urn_text("Building", "019-23-1"),
                "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-23-1")},
            },
            {
                "id": namespace.urn_text("Building", "019-23-2"),
                "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "019-23-2")},
            },
            {
                "id": namespace.urn_text("City", "019-23-1"),
                "type": "City",
                "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
                "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "019-23-1")},
            },
            {
                "id": namespace.urn_text("City", "019-23-2"),
                "type": "City",
                "description": {"type": "Property", "value": "Paris is the capital and largest city of France"},
                "isInCountry": {"type": "Relationship", "object": namespace.urn_text("Country", "019-23-2")},
            },
            {
                "id": namespace.urn_text("Country", "019-23-1"),
                "type": "Country",
                "name": {"type": "Property", "value": "France"},
            },
            {
                "id": namespace.urn_text("Country", "019-23-2"),
                "type": "Country",
                "name": {"type": "Property", "value": "France"},
            },
        ]))
        .hiding("instanceId")
    );
    delete_building_city_country_pairs(&broker, &namespace, PAIR).await?;
    Ok(())
}
