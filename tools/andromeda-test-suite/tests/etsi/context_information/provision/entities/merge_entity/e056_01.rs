//! ETSI `056_01`: an entity is merged with a fragment that changes its attributes, adds a relationship or a
//! multi-valued relationship, or holds nothing (clauses 5.6.17 and 6.5.3.4).

use crate::{
    suite_keywords::{create_entity, delete_entities, ld_json_body_client, merge_entity, retrieve_entity_json},
    suite_payloads::{BUILDING_REPLACE_TERMS, BUILDING_SIMPLE_TERMS, bare_building, building_located_in_paris, building_simple_attributes},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Entity, Input, NgsiLdMediaType, Property, Relationship, Value},
};
use serde_json::json;

/// The Building `urn:ngsi-ld:Building:<ns>-1` with the name Pisa Tower, the subCategory tourism, an `airQualityLevel`
/// of 6 observed at 2020-10-10T16:40:00Z in C62 and an `almostFull` of true.
fn updated_building(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .property("name", "Pisa Tower")
        .property("subCategory", "tourism")
        .property(
            "airQualityLevel",
            Property::builder(6).unit_code(UnitCode::C62).observed_at("2020-10-10T16:40:00.000Z".parse()?),
        )
        .property("almostFull", Value::Boolean(true))
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose `locatedAt` Relationship holds the Cities `<ns>-Paris` and
/// `<ns>-Lyon`.
fn building_located_at_paris_and_lyon(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .relationship(
            "locatedAt",
            Relationship::builder(vec![namespace.urn("City", "Paris")?, namespace.urn("City", "Lyon")?]),
        )
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose only attribute is an `airQualityLevel` of 1 observed at
/// 2024-01-01T00:00:00Z, with no unit code.
fn building_air_quality_without_unit(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .property("airQualityLevel", Property::builder(1).observed_at("2024-01-01T00:00:00.000Z".parse()?))
        .build()?)
}

/// Pins: Check that one can merge an entity
/// Clause: 5.6.17, 6.5.3.4
#[gtest]
#[tokio::test]
async fn e056_01_01_merge_an_empty_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = bare_building(&namespace, 1)?.into_fragment();
    merge_entity(&ld_json_body_client(&broker, &vocabulary)?, &original.id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": original.id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "airQualityLevel": {
                "type": "Property",
                "value": 4,
                "unitCode": "C62",
                "observedAt": "2020-09-09T16:40:00Z",
            },
            "almostFull": {"type": "Property", "value": false},
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can merge an entity
/// Clause: 5.6.17, 6.5.3.4
#[gtest]
#[tokio::test]
async fn e056_01_02_merge_simple_properties() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = updated_building(&namespace)?.into_fragment();
    merge_entity(&ld_json_body_client(&broker, &vocabulary)?, &original.id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": original.id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Pisa Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "airQualityLevel": {
                "type": "Property",
                "value": 6,
                "unitCode": "C62",
                "observedAt": "2020-10-10T16:40:00Z",
            },
            "almostFull": {"type": "Property", "value": true},
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can merge an entity
/// Clause: 5.6.17, 6.5.3.4
#[gtest]
#[tokio::test]
async fn e056_01_03_merge_simple_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_REPLACE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = building_located_in_paris(&namespace, "1")?.into_fragment();
    merge_entity(&ld_json_body_client(&broker, &vocabulary)?, &original.id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": original.id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "airQualityLevel": {
                "type": "Property",
                "value": 4,
                "unitCode": "C62",
                "observedAt": "2020-09-09T16:40:00Z",
            },
            "almostFull": {"type": "Property", "value": false},
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "Paris")},
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can merge an entity
/// Clause: 5.6.17, 6.5.3.4
#[gtest]
#[tokio::test]
async fn e056_01_04_merge_property_with_partial_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = building_air_quality_without_unit(&namespace)?.into_fragment();
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    merge_entity(&client, &original.id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": original.id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "airQualityLevel": {
                "type": "Property",
                "value": 1,
                "unitCode": "C62",
                "observedAt": "2024-01-01T00:00:00Z",
            },
            "almostFull": {"type": "Property", "value": false},
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can merge an entity
/// Clause: 5.6.17, 6.5.3.4
#[gtest]
#[tokio::test]
async fn e056_01_05_merge_multivalued_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_REPLACE_TERMS).await?;
    let original = building_simple_attributes(&namespace, 1)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = building_located_at_paris_and_lyon(&namespace)?.into_fragment();
    merge_entity(&ld_json_body_client(&broker, &vocabulary)?, &original.id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": original.id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "airQualityLevel": {
                "type": "Property",
                "value": 4,
                "unitCode": "C62",
                "observedAt": "2020-09-09T16:40:00Z",
            },
            "almostFull": {"type": "Property", "value": false},
            "locatedAt": {
                "type": "Relationship",
                "object": [namespace.urn_text("City", "Paris"), namespace.urn_text("City", "Lyon")],
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
