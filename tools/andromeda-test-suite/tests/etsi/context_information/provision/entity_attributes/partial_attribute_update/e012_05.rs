//! ETSI `012_05`: an attribute is deleted by a partial attribute update whose body is the NGSI-LD Null in the shape of
//! its kind, and the other attributes of the entity are kept (clauses 5.6.4 and 6.7.3.1).

use crate::suite_keywords::{create_building_located_street_geo, delete_entities, ld_json_body_client, partial_update, retrieve_entity_json};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{AttributeKind, AttributeName, Change, InstanceDeletion, Lossless, NgsiLdMediaType};
use serde_json::json;

/// Pins: Check that one can delete an attribute using NGSI-LD Null in an Partial Attribute Update operation
/// Clause: 5.6.4, 6.7.3.1
#[gtest]
#[tokio::test]
async fn e012_05_01_delete_a_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_located_street_geo(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let name: AttributeName = "name".parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update(
        &client,
        &id,
        &name,
        Change::Delete(InstanceDeletion::new(AttributeKind::Property)),
        Lossless::Normalized,
    )
    .await?;
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

/// Pins: Check that one can delete an attribute using NGSI-LD Null in an Partial Attribute Update operation
/// Clause: 5.6.4, 6.7.3.1
#[gtest]
#[tokio::test]
async fn e012_05_02_delete_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_located_street_geo(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let located_at: AttributeName = "locatedAt".parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update(
        &client,
        &id,
        &located_at,
        Change::Delete(InstanceDeletion::new(AttributeKind::Relationship)),
        Lossless::Normalized,
    )
    .await?;
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

/// Pins: Check that one can delete an attribute using NGSI-LD Null in an Partial Attribute Update operation
/// Clause: 5.6.4, 6.7.3.1
#[gtest]
#[tokio::test]
async fn e012_05_03_delete_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_located_street_geo(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let location: AttributeName = "location".parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update(
        &client,
        &id,
        &location,
        Change::Delete(InstanceDeletion::new(AttributeKind::GeoProperty)),
        Lossless::Normalized,
    )
    .await?;
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

/// Pins: Check that one can delete an attribute using NGSI-LD Null in an Partial Attribute Update operation
/// Clause: 5.6.4, 6.7.3.1, 4.5.18
#[gtest]
#[tokio::test]
async fn e012_05_04_delete_a_language_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_located_street_geo(&broker, &namespace).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let street: AttributeName = "street".parse()?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update(
        &client,
        &id,
        &street,
        Change::Delete(InstanceDeletion::new(AttributeKind::LanguageProperty)),
        Lossless::Normalized,
    )
    .await?;
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
