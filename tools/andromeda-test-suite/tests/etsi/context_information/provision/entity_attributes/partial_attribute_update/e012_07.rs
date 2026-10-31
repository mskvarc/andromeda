//! A partial attribute update deletes the default instance of an attribute, the one with no `datasetId`, with the
//! NGSI-LD Null in the shape of its kind, and keeps the other instances (clauses 5.6.4 and 6.7.3.1).

use crate::{
    building_fixtures::create_building_instances,
    entity_operations::{delete_entities, ld_json_body_client, partial_update, retrieve_entity_json},
    linked_place_payloads::district_paris_08,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{AttributeKind, AttributeName, Change, InstanceDeletion, Lossless, NgsiLdMediaType};
use serde_json::json;

/// Pins: Check that one can delete the default attribute instance using NGSI-LD Null in an Partial Attribute
/// Update operation
/// Clause: 5.6.4, 6.7.3.1
#[gtest]
#[tokio::test]
async fn e012_07_01_delete_a_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_instances(&broker, &namespace, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let name: AttributeName = "name".parse()?;
    let deleted = InstanceDeletion::new(AttributeKind::Property);
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update(&client, &id, &name, Change::Delete(deleted), Lossless::Normalized).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    let first = namespace.urn_text("Dataset", "01");
    let district = district_paris_08(&namespace)?.to_string();
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower", "datasetId": first},
            "locatedAt": [
                {"type": "Relationship", "object": namespace.urn_text("City", "Paris"), "datasetId": first},
                {"type": "Relationship", "object": district},
            ],
            "location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.3514, 48.8575]}, "datasetId": first},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.2944, 48.8585]}},
            ],
            "street": [
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Jardin de la Tour Eiffel", "en": "Eiffel Tower Garden"},
                    "datasetId": first,
                },
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Avenue Gustave Eiffel", "en": "Avenue Gustave Eiffel"},
                },
            ],
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can delete the default attribute instance using NGSI-LD Null in an Partial Attribute
/// Update operation
/// Clause: 5.6.4, 6.7.3.1
#[gtest]
#[tokio::test]
async fn e012_07_02_delete_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_instances(&broker, &namespace, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let located_at: AttributeName = "locatedAt".parse()?;
    let deleted = InstanceDeletion::new(AttributeKind::Relationship);
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update(&client, &id, &located_at, Change::Delete(deleted), Lossless::Normalized).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    let first = namespace.urn_text("Dataset", "01");
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": [
                {"type": "Property", "value": "Eiffel Tower", "datasetId": first},
                {"type": "Property", "value": "The iron lady"},
            ],
            "locatedAt": {"type": "Relationship", "object": namespace.urn_text("City", "Paris"), "datasetId": first},
            "location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.3514, 48.8575]}, "datasetId": first},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.2944, 48.8585]}},
            ],
            "street": [
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Jardin de la Tour Eiffel", "en": "Eiffel Tower Garden"},
                    "datasetId": first,
                },
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Avenue Gustave Eiffel", "en": "Avenue Gustave Eiffel"},
                },
            ],
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can delete the default attribute instance using NGSI-LD Null in an Partial Attribute
/// Update operation
/// Clause: 5.6.4, 6.7.3.1
#[gtest]
#[tokio::test]
async fn e012_07_03_delete_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_instances(&broker, &namespace, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let location: AttributeName = "location".parse()?;
    let deleted = InstanceDeletion::new(AttributeKind::GeoProperty);
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update(&client, &id, &location, Change::Delete(deleted), Lossless::Normalized).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    let first = namespace.urn_text("Dataset", "01");
    let district = district_paris_08(&namespace)?.to_string();
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": [
                {"type": "Property", "value": "Eiffel Tower", "datasetId": first},
                {"type": "Property", "value": "The iron lady"},
            ],
            "locatedAt": [
                {"type": "Relationship", "object": namespace.urn_text("City", "Paris"), "datasetId": first},
                {"type": "Relationship", "object": district},
            ],
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.3514, 48.8575]}, "datasetId": first},
            "street": [
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Jardin de la Tour Eiffel", "en": "Eiffel Tower Garden"},
                    "datasetId": first,
                },
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Avenue Gustave Eiffel", "en": "Avenue Gustave Eiffel"},
                },
            ],
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can delete the default attribute instance using NGSI-LD Null in an Partial Attribute
/// Update operation
/// Clause: 5.6.4, 6.7.3.1, 4.5.18
#[gtest]
#[tokio::test]
async fn e012_07_04_delete_a_language_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_instances(&broker, &namespace, None).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let street: AttributeName = "street".parse()?;
    let deleted = InstanceDeletion::new(AttributeKind::LanguageProperty);
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update(&client, &id, &street, Change::Delete(deleted), Lossless::Normalized).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    let first = namespace.urn_text("Dataset", "01");
    let district = district_paris_08(&namespace)?.to_string();
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": [
                {"type": "Property", "value": "Eiffel Tower", "datasetId": first},
                {"type": "Property", "value": "The iron lady"},
            ],
            "locatedAt": [
                {"type": "Relationship", "object": namespace.urn_text("City", "Paris"), "datasetId": first},
                {"type": "Relationship", "object": district},
            ],
            "location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.3514, 48.8575]}, "datasetId": first},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.2944, 48.8585]}},
            ],
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Jardin de la Tour Eiffel", "en": "Eiffel Tower Garden"},
                "datasetId": first,
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
