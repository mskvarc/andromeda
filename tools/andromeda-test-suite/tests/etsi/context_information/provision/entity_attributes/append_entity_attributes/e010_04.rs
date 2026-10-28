//! ETSI `010_04`: with `noOverwrite`, an existing multi-attribute instance is left as it is and reported as not
//! updated, and a new multi-attribute instance is added (clauses 5.6.3 and 6.6.3.1).

use crate::{
    suite_keywords::{
        append_attributes,
        create_vehicle_brand_parked_speed_sources,
        delete_entities,
        ld_json_body_client,
        non_empty_member,
        retrieve_entity_json,
    },
    suite_payloads::{VEHICLE_BRAND_PARKED_SPEED_SOURCE_ADDED_TERMS, speed_and_added_attribute},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::operation::Overwrite, model::NgsiLdMediaType};
use serde_json::{Value, json};

/// Pins: Check that one can append entity attributes with noOverwrite option
/// Clause: 5.6.3
/// Changed: the expected updated attribute name <https://uri.etsi.org/ngsi-ld/default-context/attribute_to_be_added> is
/// the per-file IRI of `attribute_to_be_added`: the suite @context does not define the term, so the Robot run expands
/// it under the core default vocabulary, while the file vocabulary maps it, as every term the case sends with a
/// @context
#[gtest]
#[tokio::test]
async fn e010_04_01_append_entity_attributes_and_ignore_existing_multi_attribute_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_ADDED_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = speed_and_added_attribute(&namespace, "speedometer")?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let answer = client
        .with_responses()
        .entities()
        .append_attributes(&id, &fragment)
        .overwrite(Overwrite::Keep)
        .send()
        .await;
    verify_that!(&answer, answered(StatusCode::MULTI_STATUS))?;
    let result = json_body(&answer?)?;
    expect_that!(
        result.pointer("/updated"),
        some(eq(&json!([namespace.expanded_term("attribute_to_be_added")?])))
    );
    expect_that!(result.pointer("/notUpdated").and_then(Value::as_array).map(Vec::len), some(eq(1)));
    expect_that!(
        result.pointer("/notUpdated/0/attributeName"),
        some(eq(&json!(namespace.expanded_term("speed")?)))
    );
    expect_that!(
        result.pointer("/notUpdated/0").and_then(|entry| non_empty_member(entry, "reason")),
        some(anything())
    );

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
            "attribute_to_be_added": {"type": "Property", "value": 23},
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can append entity attributes with noOverwrite option
/// Clause: 5.6.3
/// Changed: the response body is also checked to be empty, which the Hurl file does not check
#[gtest]
#[tokio::test]
async fn e010_04_02_append_entity_attributes_with_a_new_multi_attribute_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_ADDED_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = speed_and_added_attribute(&namespace, "speedometerA4567-speed2")?;
    append_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment, Overwrite::Keep).await?;
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
                {
                    "type": "Property",
                    "value": 56,
                    "source": {"type": "Property", "value": "Speedometer"},
                    "datasetId": namespace.urn_text("Property", "speedometerA4567-speed2"),
                },
            ],
            "attribute_to_be_added": {"type": "Property", "value": 23},
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
