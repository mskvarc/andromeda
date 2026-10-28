//! ETSI `011_08`: an update deletes the instance of an attribute that names one dataset, by writing the NGSI-LD Null in the
//! shape of the attribute's kind with that `datasetId`, and keeps the other instance (clauses 5.6.2 and 6.6.3.2).

use crate::{
    suite_keywords::{create_building_instances, delete_entities, ld_json_body_client, retrieve_entity_json, update_attributes},
    suite_payloads::{dataset, district_paris_08},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{AttributeKind, EntityFragment, NgsiLdMediaType};
use serde_json::json;

/// Pins: Check that one can delete a specific attribute instance using NGSI-LD Null in an Update Attributes operation
/// Clause: 5.6.2, 6.6.3.2
#[gtest]
#[tokio::test]
async fn e011_08_01_delete_a_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_instances(&broker, &namespace, Some("02")).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder()
        .delete_instance("name", AttributeKind::Property, dataset(&namespace, "01")?)
        .build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    let first = namespace.urn_text("Dataset", "01");
    let second = namespace.urn_text("Dataset", "02");
    let district = district_paris_08(&namespace)?.to_string();
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "The iron lady", "datasetId": second},
            "locatedAt": [
                {"type": "Relationship", "object": namespace.urn_text("City", "Paris"), "datasetId": first},
                {"type": "Relationship", "object": district, "datasetId": second},
            ],
            "location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.3514, 48.8575]}, "datasetId": first},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.2944, 48.8585]}, "datasetId": second},
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
                    "datasetId": second,
                },
            ],
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can delete a specific attribute instance using NGSI-LD Null in an Update Attributes operation
/// Clause: 5.6.2, 6.6.3.2
#[gtest]
#[tokio::test]
async fn e011_08_02_delete_a_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_instances(&broker, &namespace, Some("02")).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder()
        .delete_instance("locatedAt", AttributeKind::Relationship, dataset(&namespace, "01")?)
        .build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    let first = namespace.urn_text("Dataset", "01");
    let second = namespace.urn_text("Dataset", "02");
    let district = district_paris_08(&namespace)?.to_string();
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": [
                {"type": "Property", "value": "Eiffel Tower", "datasetId": first},
                {"type": "Property", "value": "The iron lady", "datasetId": second},
            ],
            "locatedAt": {"type": "Relationship", "object": district, "datasetId": second},
            "location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.3514, 48.8575]}, "datasetId": first},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.2944, 48.8585]}, "datasetId": second},
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
                    "datasetId": second,
                },
            ],
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can delete a specific attribute instance using NGSI-LD Null in an Update Attributes operation
/// Clause: 5.6.2, 6.6.3.2
#[gtest]
#[tokio::test]
async fn e011_08_03_delete_a_geo_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_instances(&broker, &namespace, Some("02")).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder()
        .delete_instance("location", AttributeKind::GeoProperty, dataset(&namespace, "01")?)
        .build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    let first = namespace.urn_text("Dataset", "01");
    let second = namespace.urn_text("Dataset", "02");
    let district = district_paris_08(&namespace)?.to_string();
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": [
                {"type": "Property", "value": "Eiffel Tower", "datasetId": first},
                {"type": "Property", "value": "The iron lady", "datasetId": second},
            ],
            "locatedAt": [
                {"type": "Relationship", "object": namespace.urn_text("City", "Paris"), "datasetId": first},
                {"type": "Relationship", "object": district, "datasetId": second},
            ],
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.2944, 48.8585]}, "datasetId": second},
            "street": [
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Jardin de la Tour Eiffel", "en": "Eiffel Tower Garden"},
                    "datasetId": first,
                },
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Avenue Gustave Eiffel", "en": "Avenue Gustave Eiffel"},
                    "datasetId": second,
                },
            ],
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can delete a specific attribute instance using NGSI-LD Null in an Update Attributes operation
/// Clause: 5.6.2, 6.6.3.2, 4.5.18
#[gtest]
#[tokio::test]
async fn e011_08_04_delete_a_language_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building_instances(&broker, &namespace, Some("02")).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder()
        .delete_instance("street", AttributeKind::LanguageProperty, dataset(&namespace, "01")?)
        .build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    let first = namespace.urn_text("Dataset", "01");
    let second = namespace.urn_text("Dataset", "02");
    let district = district_paris_08(&namespace)?.to_string();
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": [
                {"type": "Property", "value": "Eiffel Tower", "datasetId": first},
                {"type": "Property", "value": "The iron lady", "datasetId": second},
            ],
            "locatedAt": [
                {"type": "Relationship", "object": namespace.urn_text("City", "Paris"), "datasetId": first},
                {"type": "Relationship", "object": district, "datasetId": second},
            ],
            "location": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.3514, 48.8575]}, "datasetId": first},
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [2.2944, 48.8585]}, "datasetId": second},
            ],
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Avenue Gustave Eiffel", "en": "Avenue Gustave Eiffel"},
                "datasetId": second,
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
