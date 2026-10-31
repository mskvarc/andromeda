//! A `JsonProperty` is appended to an entity, and its `json` keeps the members it was written with, including a JSON
//! `null` (clauses 5.6.3 and 4.5.24).

use crate::{
    entity_operations::{append_attributes, create_entity, delete_entities, ld_json_body_client, retrieve_entity_json},
    vehicle_entity_payloads::vehicle_brand_parked_speed_sources,
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::Overwrite,
    model::{EntityFragment, JsonProperty, NgsiLdMediaType},
};
use serde_json::json;

/// The terms of the `@context` the case registers: the Vehicle of its setup and the `jsonProperty` it appends.
const TERMS: [&str; 7] = ["Vehicle", "brandName", "isParked", "providedBy", "speed", "source", "jsonProperty"];

/// The `json` the case appends.
const JSON_PROPERTY: &str = r#"{"id":"some-id","type":"some-type","null":null,"anythingElse":"anything-else"}"#;

/// Pins: Check that you can append a `JsonProperty` property to an entity
/// Clause: 5.6.3, 4.5.24
#[gtest]
#[tokio::test]
async fn e010_10_01_append_a_json_property() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TERMS).await?;
    let original = vehicle_brand_parked_speed_sources(&namespace)?;
    create_entity(&broker, &vocabulary, &original).await?;
    let fragment = EntityFragment::builder()
        .json_property("jsonProperty", JsonProperty::builder(JSON_PROPERTY.parse()?))
        .build()?;
    append_attributes(&ld_json_body_client(&broker, &vocabulary)?, &original.id, &fragment, Overwrite::Replace).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &original.id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": original.id.to_string(),
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
            "jsonProperty": {
                "type": "JsonProperty",
                "json": {"id": "some-id", "type": "some-type", "null": null, "anythingElse": "anything-else"},
            },
            "@context": vocabulary.url().as_str(),
        }))
    );
    delete_entities(&broker, &[&original.id]).await;
    Ok(())
}
