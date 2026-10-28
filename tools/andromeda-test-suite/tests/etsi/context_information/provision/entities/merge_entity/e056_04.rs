//! ETSI `056_04`: one instance of an attribute, told apart by its `datasetId`, is deleted by a Merge Entity whose
//! fragment holds the NGSI-LD Null with that `datasetId` (clauses 5.6.17 and 6.5.3.4).

use crate::{
    suite_keywords::{create_entity, delete_entities, merge_entity, retrieve_entity_json},
    suite_payloads::{BUILDING_LOCATED_STREET_TERMS, dataset, district_paris_08, point},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{AttributeKind, Entity, EntityFragment, GeoProperty, Input, LanguageMap, LanguageProperty, NgsiLdMediaType, Property, Relationship},
};
use serde_json::json;

/// The Building `urn:ngsi-ld:Building:<ns>-1` with two instances of each attribute: the name Eiffel Tower and The iron
/// lady, a `locatedAt` to `<ns>-Paris` and to the District of [`district_paris_08`], a `location` at 2.3514, 48.8575
/// and at 2.2944, 48.8585, and a `street` in French and English, each with its `datasetId` `<ns>-01` or `<ns>-02`.
fn building_with_two_instances(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .property("name", Property::builder("Eiffel Tower").dataset_id(dataset(namespace, "01")?))
        .property("name", Property::builder("The iron lady").dataset_id(dataset(namespace, "02")?))
        .relationship(
            "locatedAt",
            Relationship::builder(namespace.urn("City", "Paris")?).dataset_id(dataset(namespace, "01")?),
        )
        .relationship(
            "locatedAt",
            Relationship::builder(district_paris_08(namespace)?).dataset_id(dataset(namespace, "02")?),
        )
        .geo_property("location", GeoProperty::builder(point(2.3514, 48.8575)?).dataset_id(dataset(namespace, "01")?))
        .geo_property("location", GeoProperty::builder(point(2.2944, 48.8585)?).dataset_id(dataset(namespace, "02")?))
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([
                ("fr", "Jardin de la Tour Eiffel"),
                ("en", "Eiffel Tower Garden"),
            ])?)
            .dataset_id(dataset(namespace, "01")?),
        )
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Avenue Gustave Eiffel"), ("en", "Avenue Gustave Eiffel")])?)
                .dataset_id(dataset(namespace, "02")?),
        )
        .build()?)
}

/// Pins: Check that one can delete a specific attribute instance using NGSI-LD Null in a Merge Entity operation
/// Clause: 5.6.17, 6.5.3.4, 4.5.5
#[gtest]
#[tokio::test]
async fn e056_04_01_delete_a_property_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let original = building_with_two_instances(&namespace)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .delete_instance("name", AttributeKind::Property, dataset(&namespace, "01")?)
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
            "name": {
                "type": "Property",
                "value": "The iron lady",
                "datasetId": namespace.urn_text("Dataset", "02"),
            },
            "locatedAt": [
                {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "Paris"),
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {
                    "type": "Relationship",
                    "object": district_paris_08(&namespace)?.to_string(),
                    "datasetId": namespace.urn_text("Dataset", "02"),
                },
            ],
            "location": [
                {
                    "type": "GeoProperty",
                    "value": {"type": "Point", "coordinates": [2.3514, 48.8575]},
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {
                    "type": "GeoProperty",
                    "value": {"type": "Point", "coordinates": [2.2944, 48.8585]},
                    "datasetId": namespace.urn_text("Dataset", "02"),
                },
            ],
            "street": [
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Jardin de la Tour Eiffel", "en": "Eiffel Tower Garden"},
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Avenue Gustave Eiffel", "en": "Avenue Gustave Eiffel"},
                    "datasetId": namespace.urn_text("Dataset", "02"),
                },
            ],
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can delete a specific attribute instance using NGSI-LD Null in a Merge Entity operation
/// Clause: 5.6.17, 6.5.3.4, 4.5.5
#[gtest]
#[tokio::test]
async fn e056_04_02_delete_a_relationship_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let original = building_with_two_instances(&namespace)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .delete_instance("locatedAt", AttributeKind::Relationship, dataset(&namespace, "01")?)
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
            "name": [
                {
                    "type": "Property",
                    "value": "Eiffel Tower",
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {
                    "type": "Property",
                    "value": "The iron lady",
                    "datasetId": namespace.urn_text("Dataset", "02"),
                },
            ],
            "locatedAt": {
                "type": "Relationship",
                "object": district_paris_08(&namespace)?.to_string(),
                "datasetId": namespace.urn_text("Dataset", "02"),
            },
            "location": [
                {
                    "type": "GeoProperty",
                    "value": {"type": "Point", "coordinates": [2.3514, 48.8575]},
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {
                    "type": "GeoProperty",
                    "value": {"type": "Point", "coordinates": [2.2944, 48.8585]},
                    "datasetId": namespace.urn_text("Dataset", "02"),
                },
            ],
            "street": [
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Jardin de la Tour Eiffel", "en": "Eiffel Tower Garden"},
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Avenue Gustave Eiffel", "en": "Avenue Gustave Eiffel"},
                    "datasetId": namespace.urn_text("Dataset", "02"),
                },
            ],
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can delete a specific attribute instance using NGSI-LD Null in a Merge Entity operation
/// Clause: 5.6.17, 6.5.3.4, 4.5.5
#[gtest]
#[tokio::test]
async fn e056_04_03_delete_a_geo_property_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let original = building_with_two_instances(&namespace)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .delete_instance("location", AttributeKind::GeoProperty, dataset(&namespace, "01")?)
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
            "name": [
                {
                    "type": "Property",
                    "value": "Eiffel Tower",
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {
                    "type": "Property",
                    "value": "The iron lady",
                    "datasetId": namespace.urn_text("Dataset", "02"),
                },
            ],
            "locatedAt": [
                {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "Paris"),
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {
                    "type": "Relationship",
                    "object": district_paris_08(&namespace)?.to_string(),
                    "datasetId": namespace.urn_text("Dataset", "02"),
                },
            ],
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [2.2944, 48.8585]},
                "datasetId": namespace.urn_text("Dataset", "02"),
            },
            "street": [
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Jardin de la Tour Eiffel", "en": "Eiffel Tower Garden"},
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {
                    "type": "LanguageProperty",
                    "languageMap": {"fr": "Avenue Gustave Eiffel", "en": "Avenue Gustave Eiffel"},
                    "datasetId": namespace.urn_text("Dataset", "02"),
                },
            ],
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can delete a specific attribute instance using NGSI-LD Null in a Merge Entity operation
/// Clause: 5.6.17, 6.5.3.4, 4.5.18, 4.5.5
#[gtest]
#[tokio::test]
async fn e056_04_04_delete_a_language_property_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let original = building_with_two_instances(&namespace)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .delete_instance("street", AttributeKind::LanguageProperty, dataset(&namespace, "01")?)
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
            "name": [
                {
                    "type": "Property",
                    "value": "Eiffel Tower",
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {
                    "type": "Property",
                    "value": "The iron lady",
                    "datasetId": namespace.urn_text("Dataset", "02"),
                },
            ],
            "locatedAt": [
                {
                    "type": "Relationship",
                    "object": namespace.urn_text("City", "Paris"),
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {
                    "type": "Relationship",
                    "object": district_paris_08(&namespace)?.to_string(),
                    "datasetId": namespace.urn_text("Dataset", "02"),
                },
            ],
            "location": [
                {
                    "type": "GeoProperty",
                    "value": {"type": "Point", "coordinates": [2.3514, 48.8575]},
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {
                    "type": "GeoProperty",
                    "value": {"type": "Point", "coordinates": [2.2944, 48.8585]},
                    "datasetId": namespace.urn_text("Dataset", "02"),
                },
            ],
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Avenue Gustave Eiffel", "en": "Avenue Gustave Eiffel"},
                "datasetId": namespace.urn_text("Dataset", "02"),
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
