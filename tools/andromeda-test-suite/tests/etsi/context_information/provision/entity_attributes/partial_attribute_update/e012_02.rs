//! ETSI `012_02`: a partial attribute update is refused with 400 `BadRequestData` when its entity id is missing or not
//! a URI, when its body names another attribute type, when its fragment is empty, and when its `datasetId` is the
//! NGSI-LD Null (clause 5.6.4).

use crate::{
    suite_keywords::{create_vehicle_brand_parked_speed_gps, delete_entities, ld_json_body_client, refused},
    suite_payloads::{VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS, speedometer_speed_56_patch},
};
use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    wire::{api_uri, content_type},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{UncheckedDocument, operation::PartialAttributeUpdate},
    model::{APPLICATION_LD_JSON, Attribute, AttributeName, Change, ErrorType, Lossless, Property, Relationship},
};

/// Pins: Check that one cannot perform a partial update on an entity attribute with missing entity id
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn e012_02_01_make_a_partial_attribute_update_if_the_entity_id_is_not_present() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let change = Change::Set(speedometer_speed_56_patch(&namespace)?);
    let operation = PartialAttributeUpdate::builder()
        .id(&id)
        .name(&speed)
        .attribute(&change)
        .form(Lossless::Concise)
        .build();
    let request = client.unchecked(&operation)?.uri(api_uri(&client, "entities//attrs/speed")?);
    refused(&client, request, ErrorType::BadRequestData).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot perform a partial update on an entity attribute with invalid entity id
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn e012_02_02_make_a_partial_attribute_update_if_the_entity_id_is_not_a_valid_uri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let change = Change::Set(speedometer_speed_56_patch(&namespace)?);
    let operation = PartialAttributeUpdate::builder()
        .id(&id)
        .name(&speed)
        .attribute(&change)
        .form(Lossless::Concise)
        .build();
    let request = client.unchecked(&operation)?.uri(api_uri(&client, "entities/thisisaninvaliduri/attrs/speed")?);
    refused(&client, request, ErrorType::BadRequestData).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot perform a partial update on an entity attribute with a different attribute type
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn e012_02_03_make_a_partial_attribute_update_if_the_attribute_type_does_not_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let relationship = Attribute::from(Relationship::builder(namespace.urn("Entity", "01")?).build()?).into_patch();
    let change = Change::Set(relationship);
    let operation = PartialAttributeUpdate::builder().id(&id).name(&speed).attribute(&change).build();
    refused(&client, client.unchecked(&operation)?, ErrorType::BadRequestData).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot perform a partial update on an entity attribute with an empty fragment
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn e012_02_04_make_a_partial_attribute_update_if_the_entity_fragment_is_empty() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let change = Change::Set(speedometer_speed_56_patch(&namespace)?);
    let operation = PartialAttributeUpdate::builder().id(&id).name(&speed).attribute(&change).build();
    let request = content_type(broker.unchecked(&operation)?.document_as_is(&UncheckedDocument::empty()), &APPLICATION_LD_JSON)?;
    refused(&broker, request, ErrorType::BadRequestData).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot perform a partial update on an entity attribute with a null datasetId
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn e012_02_05_partial_update_setting_dataset_id_to_null() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let attribute = Attribute::from(Property::builder(56).build()?).into_patch();
    let document = UncheckedDocument::try_from(attribute.render(Lossless::Normalized))?.set_text("datasetId", "urn:ngsi-ld:null");
    let change = Change::Set(attribute);
    let operation = PartialAttributeUpdate::builder().id(&id).name(&speed).attribute(&change).build();
    refused(&client, client.unchecked(&operation)?.document(&document), ErrorType::BadRequestData).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
