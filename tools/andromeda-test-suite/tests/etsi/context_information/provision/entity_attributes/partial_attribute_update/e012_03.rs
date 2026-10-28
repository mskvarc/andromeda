//! ETSI `012_03`: a partial attribute update is refused with 404 `ResourceNotFound` when its entity is not known, when
//! the attribute has no default instance or no instance with the `datasetId` it names, and when the attribute is not
//! in the entity (clause 5.6.4).

use crate::{
    suite_keywords::{create_vehicle_brand_parked_speed_sources, delete_entities, ld_json_body_client, not_found, partial_update_request},
    suite_payloads::{VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS, provided_by_john_doe},
};
use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::operation::PartialAttributeUpdate,
    model::{Attribute, AttributeName, Change, DatasetId, Patch, Property},
};

/// The `speed` of 56 from a Speedometer, with no `datasetId`.
fn speedometer_56_without_dataset() -> Result<Attribute<Patch>> {
    Ok(Attribute::from(Property::builder(56).property("source", Property::builder("Speedometer")).build()?).into_patch())
}

/// The `speed` of 56 from a Speedometer, with the `datasetId` `urn:ngsi-ld:Property:<ns>-unknownDatasetId`.
fn speedometer_56_in_unknown_dataset(namespace: &Namespace) -> Result<Attribute<Patch>> {
    Ok(Attribute::from(
        Property::builder(56)
            .property("source", Property::builder("Speedometer"))
            .dataset_id(DatasetId::from(namespace.urn("Property", "unknownDatasetId")?.as_uri().clone()))
            .build()?,
    )
    .into_patch())
}

/// Pins: Check that one cannot perform a partial update on an entity attribute if the entity id or attribute is not
/// known to the system
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn e012_03_01_partial_update_when_the_entity_id_is_not_known_to_the_system() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let id = namespace.urn("Vehicle", 1)?;
    let unknown = namespace.urn("Vehicle", 2)?;
    let speed: AttributeName = "speed".parse()?;
    let request = partial_update_request(&client, &unknown, &speed, &provided_by_john_doe(&namespace)?)?;
    not_found(&client, request).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot perform a partial update on an entity attribute if the entity id or attribute is not
/// known to the system
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn e012_03_02_partial_update_when_no_default_instance_and_no_dataset_id_specified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let change = Change::Set(speedometer_56_without_dataset()?);
    let operation = PartialAttributeUpdate::builder().id(&id).name(&speed).attribute(&change).build();
    not_found(&client, client.unchecked(&operation)?).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot perform a partial update on an entity attribute if the entity id or attribute is not
/// known to the system
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn e012_03_03_partial_update_when_no_instance_with_the_dataset_id_specified() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let id = namespace.urn("Vehicle", 1)?;
    let speed: AttributeName = "speed".parse()?;
    let change = Change::Set(speedometer_56_in_unknown_dataset(&namespace)?);
    let operation = PartialAttributeUpdate::builder().id(&id).name(&speed).attribute(&change).build();
    not_found(&client, client.unchecked(&operation)?).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one cannot perform a partial update on an entity attribute if the entity id or attribute is not
/// known to the system
/// Clause: 5.6.4
#[gtest]
#[tokio::test]
async fn e012_03_04_partial_update_when_the_attribute_name_does_not_exist_in_the_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_sources(
        &broker,
        &namespace,
        &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source", "isParked2"],
    )
    .await?;
    let client = ld_json_body_client(&broker, &vocabulary)?;
    let id = namespace.urn("Vehicle", 1)?;
    let missing: AttributeName = "isParked2".parse()?;
    let request = partial_update_request(&client, &id, &missing, &provided_by_john_doe(&namespace)?)?;
    not_found(&client, request).await?;
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
