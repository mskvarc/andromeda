//! ETSI `056_07`: sub-attributes set to the NGSI-LD Null in a Merge Entity are removed from the Property they belong to
//! (clauses 5.6.17 and 6.5.3.4).

use crate::suite_keywords::{OBSERVED_AT_WHOLE_SECOND, create_entity, delete_entities, merge_entity, observed_at, retrieve_entity_json};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::ContextDelivery,
    model::{Attribute, Entity, EntityFragment, Input, NgsiLdMediaType, PatchMember, Property, Relationship, Value},
};
use serde_json::json;

/// The terms of the `@context` the cases register.
const TERMS: [&str; 6] = ["Building", "name", "subCategory", "airQualityLevel", "hasWeatherObserved", "almostFull"];

/// The suffix of the Sensor the `hasWeatherObserved` Relationship names.
const SENSOR_SUFFIX: &str = "43a1af27-4aa7-44fe-bee9-fe76d56978a4";

/// The Building `urn:ngsi-ld:Building:<ns>-1` of the setup: an `airQualityLevel` of 4 in C62 observed at
/// 2020-09-09T16:40:00.000Z with a `hasWeatherObserved` Relationship to the Sensor `urn:ngsi-ld:Sensor:<ns>-<suffix>`.
fn building_with_weather_sensor(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .property(
            "airQualityLevel",
            Property::builder(4)
                .unit_code(UnitCode::C62)
                .observed_at("2020-09-09T16:40:00.000Z".parse()?)
                .relationship("hasWeatherObserved", Relationship::builder(namespace.urn("Sensor", SENSOR_SUFFIX)?)),
        )
        .property("almostFull", Value::Boolean(false))
        .build()?)
}

/// Pins: Check that sub-attributes set to NGSI-LD Null in a Merge Entity operation are removed
/// Clause: 5.6.17, 6.5.3.4
/// Changed: the body is compared as one exact document, where the Hurl file checks its members one by one
#[gtest]
#[tokio::test]
async fn e056_07_01_remove_unit_code_from_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let original = building_with_weather_sensor(&namespace)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let unit_code_removed = Attribute::from(Property::builder(4).build()?)
        .into_patch()
        .delete_member(PatchMember::UnitCode)?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .patch("airQualityLevel", unit_code_removed)
        .build()?;
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    merge_entity(&client, &original.id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "airQualityLevel": {
                "type": "Property",
                "value": 4,
                "observedAt": "2020-09-09T16:40:00.000Z",
                "hasWeatherObserved": {
                    "type": "Relationship",
                    "object": namespace.urn_text("Sensor", SENSOR_SUFFIX),
                },
            },
            "almostFull": {"type": "Property", "value": false},
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    expect_that!(observed_at(&body, "airQualityLevel"), some(matches_regex(OBSERVED_AT_WHOLE_SECOND)));
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}

/// Pins: Check that sub-attributes set to NGSI-LD Null in a Merge Entity operation are removed
/// Clause: 5.6.17, 6.5.3.4
/// Changed: the body is compared as one exact document, where the Hurl file checks its members one by one
#[gtest]
#[tokio::test]
async fn e056_07_02_remove_sub_attribute_from_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let original = building_with_weather_sensor(&namespace)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::for_entity(namespace.urn("Building", 1)?, "Building")
        .property("airQualityLevel", Property::builder(4))
        .delete_sub_attribute("airQualityLevel", "hasWeatherObserved")
        .build()?;
    let client = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    merge_entity(&client, &original.id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": namespace.urn_text("Building", 1),
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"},
            "subCategory": {"type": "Property", "value": "tourism"},
            "airQualityLevel": {
                "type": "Property",
                "value": 4,
                "unitCode": "C62",
                "observedAt": "2020-09-09T16:40:00.000Z",
            },
            "almostFull": {"type": "Property", "value": false},
            "@context": vocabulary.url().as_str(),
        }))
        .hiding("instanceId")
    );
    expect_that!(observed_at(&body, "airQualityLevel"), some(matches_regex(OBSERVED_AT_WHOLE_SECOND)));
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
