//! ETSI `012_10`: a partial attribute update that sets a sub-attribute of an `airQualityLevel` to the NGSI-LD Null
//! removes that sub-attribute and keeps the others (clauses 5.6.4 and 6.7.3.1).

use crate::{
    suite_keywords::{create_entity, delete_entities, ld_json_body_client, partial_update, partial_update_document, retrieve_entity_json},
    suite_payloads::{BUILDING_AIR_QUALITY_WEATHER_TERMS, building_with_air_quality},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::matches_document,
    namespace::Namespace,
    vocabulary::Vocabulary,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::UncheckedDocument,
    model::{Attribute, AttributeKind, AttributeName, Change, Entity, Input, InstanceDeletion, Lossless, NgsiLdMediaType, Patch, Property, Relationship},
};
use serde_json::json;

/// The Building `urn:ngsi-ld:Building:<ns>-1` of the setup: an `airQualityLevel` of 4 in C62, observed at
/// 2020-09-09T16:40:00Z, with a `hasWeatherObserved` Relationship to the Sensor `urn:ngsi-ld:Sensor:<ns>-43a1af27-...`.
fn building_with_weather(namespace: &Namespace) -> Result<Entity<Input>> {
    building_with_air_quality(
        namespace,
        1,
        Property::builder(4)
            .unit_code(UnitCode::C62)
            .observed_at("2020-09-09T16:40:00.000Z".parse()?)
            .relationship(
                "hasWeatherObserved",
                Relationship::builder(namespace.urn("Sensor", "43a1af27-4aa7-44fe-bee9-fe76d56978a4")?),
            ),
    )
}

/// The `airQualityLevel` the partial updates of the cases write: a Property of 4 with no sub-attribute.
fn value_four_patch() -> Result<Attribute<Patch>> {
    Ok(Attribute::from(Property::builder(4).build()?).into_patch())
}

/// Registers the terms of the cases and creates the Building of [`building_with_weather`] in their context.
async fn create_building(broker: &BrokerClient, namespace: &Namespace) -> Result<Vocabulary> {
    let vocabulary = Vocabulary::register(broker, namespace, &BUILDING_AIR_QUALITY_WEATHER_TERMS).await?;
    create_entity(broker, &vocabulary, &building_with_weather(namespace)?).await?;
    Ok(vocabulary)
}

/// Pins: Check that sub-attributes set to NGSI-LD Null in a Partial Attribute Update are removed
/// Clause: 5.6.4, 6.7.3.1
#[gtest]
#[tokio::test]
async fn e012_10_01_remove_observed_at_from_air_quality_level() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building(&broker, &namespace).await?;
    let id = namespace.urn("Building", 1)?;
    let air_quality: AttributeName = "airQualityLevel".parse()?;
    let document = UncheckedDocument::try_from(value_four_patch()?.render(Lossless::Normalized))?.null_member("observedAt");
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update_document(&client, &id, &air_quality, &document).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    let sensor = namespace.urn_text("Sensor", "43a1af27-4aa7-44fe-bee9-fe76d56978a4");
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "airQualityLevel": {
                "type": "Property",
                "value": 4,
                "unitCode": "C62",
                "hasWeatherObserved": {"type": "Relationship", "object": sensor},
            },
            "almostFull": {"type": "Property", "value": false},
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that sub-attributes set to NGSI-LD Null in a Partial Attribute Update are removed
/// Clause: 5.6.4, 6.7.3.1
#[gtest]
#[tokio::test]
async fn e012_10_02_remove_unit_code_from_air_quality_level() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building(&broker, &namespace).await?;
    let id = namespace.urn("Building", 1)?;
    let air_quality: AttributeName = "airQualityLevel".parse()?;
    let document = UncheckedDocument::try_from(value_four_patch()?.render(Lossless::Normalized))?.null_member("unitCode");
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update_document(&client, &id, &air_quality, &document).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    let sensor = namespace.urn_text("Sensor", "43a1af27-4aa7-44fe-bee9-fe76d56978a4");
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "airQualityLevel": {
                "type": "Property",
                "value": 4,
                "observedAt": "2020-09-09T16:40:00Z",
                "hasWeatherObserved": {"type": "Relationship", "object": sensor},
            },
            "almostFull": {"type": "Property", "value": false},
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that sub-attributes set to NGSI-LD Null in a Partial Attribute Update are removed
/// Clause: 5.6.4, 6.7.3.1
#[gtest]
#[tokio::test]
async fn e012_10_03_remove_sub_attribute_from_air_quality_level() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_building(&broker, &namespace).await?;
    let id = namespace.urn("Building", 1)?;
    let air_quality: AttributeName = "airQualityLevel".parse()?;
    let removal = value_four_patch()?.delete_sub_attribute("hasWeatherObserved", InstanceDeletion::new(AttributeKind::Relationship))?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    partial_update(&client, &id, &air_quality, Change::Set(removal), Lossless::Normalized).await?;
    let read = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&read, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "airQualityLevel": {
                "type": "Property",
                "value": 4,
                "observedAt": "2020-09-09T16:40:00Z",
                "unitCode": "C62",
            },
            "almostFull": {"type": "Property", "value": false},
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
