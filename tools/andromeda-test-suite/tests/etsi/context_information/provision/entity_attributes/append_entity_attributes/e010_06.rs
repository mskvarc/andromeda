//! ETSI `010_06`: a `LanguageProperty` is appended to an entity, and its `languageMap` is kept as written (clauses
//! 5.6.3 and 4.5.18).

use crate::suite_keywords::{append_attributes, create_vehicle_brand_parked_speed_sources, delete_entities, ld_json_body_client, retrieve_entity_json};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::Overwrite,
    model::{EntityFragment, NgsiLdMediaType},
};
use serde_json::json;

/// The terms of the `@context` the case registers: the Vehicle of its setup and the `street` it appends.
const TERMS: [&str; 7] = ["Vehicle", "brandName", "isParked", "providedBy", "speed", "source", "street"];

/// Pins: Check that one can append a `LanguageProperty` property to an entity
/// Clause: 5.6.3, 4.5.18
#[gtest]
#[tokio::test]
async fn e010_06_01_append_a_language_property_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder()
        .language_property("street", [("fr", "Grand Place"), ("nl", "Grote Markt")])
        .build()?;
    append_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment, Overwrite::Replace).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "brandName": {"type": "Property", "value": "Mercedes"},
            "isParked": {
                "type": "Relationship",
                "object": namespace.urn_text("OffStreetParking", "Downtown1"),
                "observedAt": "2017-07-29T12:00:04Z",
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
            },
            "speed": [
                {
                    "type": "Property",
                    "value": 55,
                    "source": {"type": "Property", "value": "Speedometer"},
                    "datasetId": namespace.urn_text("Property", "speedometer"),
                },
                {
                    "type": "Property",
                    "value": 54.5,
                    "source": {"type": "Property", "value": "GPS"},
                    "datasetId": namespace.urn_text("Property", "gpsBxyz123-speed"),
                },
            ],
            "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
