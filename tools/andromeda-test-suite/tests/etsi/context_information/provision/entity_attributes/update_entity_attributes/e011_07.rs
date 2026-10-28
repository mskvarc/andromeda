//! ETSI `011_07`: an update deletes an attribute of each kind by writing the NGSI-LD Null in the shape of that kind, and
//! keeps the other attributes of the entity (clauses 5.6.2 and 6.6.3.2).

use crate::suite_keywords::{create_building_located_street_geo, delete_entities, ld_json_body_client, retrieve_entity_json, update_attributes};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{AttributeKind, EntityFragment, NgsiLdMediaType};
use serde_json::json;

/// Pins: Check that one can delete an attribute using NGSI-LD Null in an Update Attributes operation
/// Clause: 5.6.2, 6.6.3.2
#[gtest]
#[tokio::test]
async fn e011_07_01_delete_a_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_located_street_geo(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder().delete_kind("name", AttributeKind::Property).build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "Paris")},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"},
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can delete an attribute using NGSI-LD Null in an Update Attributes operation
/// Clause: 5.6.2, 6.6.3.2
#[gtest]
#[tokio::test]
async fn e011_07_02_delete_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_located_street_geo(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder().delete_kind("locatedAt", AttributeKind::Relationship).build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"},
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can delete an attribute using NGSI-LD Null in an Update Attributes operation
/// Clause: 5.6.2, 6.6.3.2
#[gtest]
#[tokio::test]
async fn e011_07_03_delete_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_located_street_geo(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder().delete_kind("location", AttributeKind::GeoProperty).build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
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
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can delete an attribute using NGSI-LD Null in an Update Attributes operation
/// Clause: 5.6.2, 6.6.3.2, 4.5.18
#[gtest]
#[tokio::test]
async fn e011_07_04_delete_a_language_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_located_street_geo(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder().delete_kind("street", AttributeKind::LanguageProperty).build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "Paris")},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
