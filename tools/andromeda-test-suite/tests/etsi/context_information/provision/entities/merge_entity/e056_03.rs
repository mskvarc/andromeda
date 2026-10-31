//! An attribute is deleted by a Merge Entity whose fragment holds the NGSI-LD Null in the shape of its kind (clauses
//! 5.6.17 and 6.5.3.4).

use crate::{
    building_payloads::{BUILDING_LOCATED_STREET_TERMS, building_located_street_geo},
    entity_operations::{create_entity, delete_entities, merge_entity, retrieve_entity_json},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{AttributeKind, EntityFragment, NgsiLdMediaType},
};
use serde_json::json;

/// Pins: Check that one can delete an attribute using NGSI-LD Null in a Merge Entity operation
/// Clause: 5.6.17, 6.5.3.4, 4.5.5
#[gtest]
#[tokio::test]
async fn e056_03_01_delete_a_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let original = building_located_street_geo(&namespace, namespace.urn("Building", 1)?)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .delete_kind("name", AttributeKind::Property)
        .build()?;
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    merge_entity(&client, &original.id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": original.id.to_string(),
            "type": "Building",
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "Paris")},
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [-8.5, 41.2]},
            },
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"},
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can delete an attribute using NGSI-LD Null in a Merge Entity operation
/// Clause: 5.6.17, 6.5.3.4
#[gtest]
#[tokio::test]
async fn e056_03_02_delete_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let original = building_located_street_geo(&namespace, namespace.urn("Building", 1)?)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .delete_kind("locatedAt", AttributeKind::Relationship)
        .build()?;
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
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [-8.5, 41.2]},
            },
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"},
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can delete an attribute using NGSI-LD Null in a Merge Entity operation
/// Clause: 5.6.17, 6.5.3.4
#[gtest]
#[tokio::test]
async fn e056_03_03_delete_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let original = building_located_street_geo(&namespace, namespace.urn("Building", 1)?)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .delete_kind("location", AttributeKind::GeoProperty)
        .build()?;
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
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "Paris")},
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"},
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can delete an attribute using NGSI-LD Null in a Merge Entity operation
/// Clause: 5.6.17, 6.5.3.4, 4.5.18
#[gtest]
#[tokio::test]
async fn e056_03_04_delete_a_language_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let original = building_located_street_geo(&namespace, namespace.urn("Building", 1)?)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .delete_kind("street", AttributeKind::LanguageProperty)
        .build()?;
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
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "Paris")},
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [-8.5, 41.2]},
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
