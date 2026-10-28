//! ETSI `056_05`: the default instance of an attribute, the one with no `datasetId`, is deleted by a Merge Entity whose
//! fragment holds the NGSI-LD Null without a `datasetId` (clauses 5.6.17 and 6.5.3.4).

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

/// The Building `urn:ngsi-ld:Building:<ns>-1` with a `datasetId` `<ns>-01` on the first instance of each attribute and
/// none on the second: the name Eiffel Tower and The iron lady, a `locatedAt` to `<ns>-Paris` and to the District of
/// [`district_paris_08`], a `location` at 2.3514, 48.8575 and at 2.2944, 48.8585, and a `street` in French and English.
fn building_with_default_instances(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .property("name", Property::builder("Eiffel Tower").dataset_id(dataset(namespace, "01")?))
        .property("name", "The iron lady")
        .relationship(
            "locatedAt",
            Relationship::builder(namespace.urn("City", "Paris")?).dataset_id(dataset(namespace, "01")?),
        )
        .relationship("locatedAt", Relationship::builder(district_paris_08(namespace)?))
        .geo_property("location", GeoProperty::builder(point(2.3514, 48.8575)?).dataset_id(dataset(namespace, "01")?))
        .geo_property("location", GeoProperty::builder(point(2.2944, 48.8585)?))
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
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Avenue Gustave Eiffel"), ("en", "Avenue Gustave Eiffel")])?),
        )
        .build()?)
}

/// Pins: Check that one can delete the default attribute instance using NGSI-LD Null in a Merge Entity operation
/// Clause: 5.6.17, 6.5.3.4, 4.5.5
#[gtest]
#[tokio::test]
async fn e056_05_01_delete_a_property_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let original = building_with_default_instances(&namespace)?;
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
            "name": {
                "type": "Property",
                "value": "Eiffel Tower",
                "datasetId": namespace.urn_text("Dataset", "01"),
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
                },
            ],
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can delete the default attribute instance using NGSI-LD Null in a Merge Entity operation
/// Clause: 5.6.17, 6.5.3.4, 4.5.5
#[gtest]
#[tokio::test]
async fn e056_05_02_delete_a_relationship_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let original = building_with_default_instances(&namespace)?;
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
            "name": [
                {
                    "type": "Property",
                    "value": "Eiffel Tower",
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {"type": "Property", "value": "The iron lady"},
            ],
            "locatedAt": {
                "type": "Relationship",
                "object": namespace.urn_text("City", "Paris"),
                "datasetId": namespace.urn_text("Dataset", "01"),
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
                },
            ],
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can delete the default attribute instance using NGSI-LD Null in a Merge Entity operation
/// Clause: 5.6.17, 6.5.3.4, 4.5.5
#[gtest]
#[tokio::test]
async fn e056_05_03_delete_a_geo_property_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let original = building_with_default_instances(&namespace)?;
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
            "name": [
                {
                    "type": "Property",
                    "value": "Eiffel Tower",
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {"type": "Property", "value": "The iron lady"},
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
                },
            ],
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [2.3514, 48.8575]},
                "datasetId": namespace.urn_text("Dataset", "01"),
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
                },
            ],
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that one can delete the default attribute instance using NGSI-LD Null in a Merge Entity operation
/// Clause: 5.6.17, 6.5.3.4, 4.5.18, 4.5.5
#[gtest]
#[tokio::test]
async fn e056_05_04_delete_a_language_property_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_LOCATED_STREET_TERMS).await?;
    let original = building_with_default_instances(&namespace)?;
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
            "name": [
                {
                    "type": "Property",
                    "value": "Eiffel Tower",
                    "datasetId": namespace.urn_text("Dataset", "01"),
                },
                {"type": "Property", "value": "The iron lady"},
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
                },
            ],
            "street": {
                "type": "LanguageProperty",
                "languageMap": {"fr": "Jardin de la Tour Eiffel", "en": "Eiffel Tower Garden"},
                "datasetId": namespace.urn_text("Dataset", "01"),
            },
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
