//! ETSI `011_01`: an update changes the attributes it names, the instance of a dataset it names, the targets of a
//! Relationship and the type of an attribute, and appends the attributes the entity does not hold (clause 5.6.2).

use crate::{
    suite_keywords::{create_vehicle_brand_parked_speed_gps, delete_entities, ld_json_body_client, retrieve_entity_json, update_attributes},
    suite_payloads::{VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS, gps_speed_dataset, parked_dataset, speedometer_speed_99_fragment},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{EntityFragment, Input, NgsiLdMediaType, Property, Relationship};
use serde_json::json;

/// The `isParked` Relationship that the update cases send: the `OffStreetParking` Downtown1 observed on 2017-07-29 at
/// 12:00:04 UTC, provided by `JohnDoe`, in the dataset of [`parked_dataset`].
fn parked_provided_by_john_doe(namespace: &Namespace) -> Result<Relationship<Input>> {
    Ok(Relationship::builder(namespace.urn("OffStreetParking", "Downtown1")?)
        .observed_at("2017-07-29T12:00:04Z".parse()?)
        .relationship("providedBy", namespace.urn("Person", "JohnDoe")?)
        .dataset_id(parked_dataset(namespace)?)
        .build()?)
}

/// The `isParked` Relationship that the multiple-objects case sends: the `OffStreetParking` Downtown1 and Downtown2,
/// observed on 2017-07-29 at 12:00:04 UTC, provided by Bob, in the dataset of [`parked_dataset`].
fn parked_at_two_downtowns(namespace: &Namespace) -> Result<Relationship<Input>> {
    Ok(Relationship::builder(vec![
        namespace.urn("OffStreetParking", "Downtown1")?,
        namespace.urn("OffStreetParking", "Downtown2")?,
    ])
    .observed_at("2017-07-29T12:00:04Z".parse()?)
    .relationship("providedBy", namespace.urn("Person", "Bob")?)
    .dataset_id(parked_dataset(namespace)?)
    .build()?)
}

/// The Speedometer `speed` of 99 that the cases write over the GPS instance, in the dataset of [`gps_speed_dataset`].
fn speed_99_over_gps(namespace: &Namespace) -> Result<Property<Input>> {
    Ok(Property::builder(99)
        .property("source", Property::builder("Speedometer"))
        .dataset_id(gps_speed_dataset(namespace)?)
        .build()?)
}

/// The `speed` instances the setup creates, which an update that does not name `speed` leaves as they are: 55 from a
/// Speedometer with no dataset, and 54.5 from a GPS in the dataset of [`gps_speed_dataset`].
fn speed_as_created(namespace: &Namespace) -> serde_json::Value {
    json!([
        {"type": "Property", "value": 55, "source": {"type": "Property", "value": "Speedometer"}},
        {
            "type": "Property",
            "value": 54.5,
            "source": {"type": "Property", "value": "GPS"},
            "datasetId": namespace.urn_text("Property", "gpsBxyz123-speed"),
        },
    ])
}

/// Pins: Check that one can update entity attributes
/// Clause: 5.6.2
#[gtest]
#[tokio::test]
async fn e011_01_01_check_that_one_can_update_existing_attributes_with_no_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &speedometer_speed_99_fragment()?).await?;
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
                "datasetId": namespace.urn_text("Relationship", "parked12345"),
            },
            "speed": [
                {"type": "Property", "value": 99, "source": {"type": "Property", "value": "Speedometer"}},
                {
                    "type": "Property",
                    "value": 54.5,
                    "source": {"type": "Property", "value": "GPS"},
                    "datasetId": namespace.urn_text("Property", "gpsBxyz123-speed"),
                },
            ],
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can update entity attributes
/// Clause: 5.6.2
#[gtest]
#[tokio::test]
async fn e011_01_02_check_that_one_can_update_existing_attributes_with_the_dataset_id() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder()
        .relationship("isParked", parked_provided_by_john_doe(&namespace)?)
        .property("speed", speed_99_over_gps(&namespace)?)
        .build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
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
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "JohnDoe")},
                "datasetId": namespace.urn_text("Relationship", "parked12345"),
            },
            "speed": [
                {"type": "Property", "value": 55, "source": {"type": "Property", "value": "Speedometer"}},
                {
                    "type": "Property",
                    "value": 99,
                    "source": {"type": "Property", "value": "Speedometer"},
                    "datasetId": namespace.urn_text("Property", "gpsBxyz123-speed"),
                },
            ],
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can update entity attributes
/// Clause: 5.6.2
#[gtest]
#[tokio::test]
async fn e011_01_03_check_that_one_can_update_existing_attributes_and_append_non_existing_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_gps(
        &broker,
        &namespace,
        &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source", "isParked2"],
    )
    .await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder()
        .relationship("isParked2", parked_provided_by_john_doe(&namespace)?)
        .property("speed", speed_99_over_gps(&namespace)?)
        .build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
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
                "datasetId": namespace.urn_text("Relationship", "parked12345"),
            },
            "isParked2": {
                "type": "Relationship",
                "object": namespace.urn_text("OffStreetParking", "Downtown1"),
                "observedAt": "2017-07-29T12:00:04Z",
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "JohnDoe")},
                "datasetId": namespace.urn_text("Relationship", "parked12345"),
            },
            "speed": [
                {"type": "Property", "value": 55, "source": {"type": "Property", "value": "Speedometer"}},
                {
                    "type": "Property",
                    "value": 99,
                    "source": {"type": "Property", "value": "Speedometer"},
                    "datasetId": namespace.urn_text("Property", "gpsBxyz123-speed"),
                },
            ],
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can update entity attributes
/// Clause: 5.6.2
#[gtest]
#[tokio::test]
async fn e011_01_04_check_that_one_can_change_the_type_of_an_existing_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder()
        .relationship("brandName", Relationship::builder(namespace.urn("Brand", "01")?).build()?)
        .build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
    let client = broker.with_context(vocabulary.context())?.with_accept(NgsiLdMediaType::JsonLd);
    let body = retrieve_entity_json(&client, &id).await?;
    expect_that!(
        &body,
        matches_document(json!({
            "id": id.to_string(),
            "type": "Vehicle",
            "brandName": {"type": "Relationship", "object": namespace.urn_text("Brand", "01")},
            "isParked": {
                "type": "Relationship",
                "object": namespace.urn_text("OffStreetParking", "Downtown1"),
                "observedAt": "2017-07-29T12:00:04Z",
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
                "datasetId": namespace.urn_text("Relationship", "parked12345"),
            },
            "speed": speed_as_created(&namespace),
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}

/// Pins: Check that one can update entity attributes
/// Clause: 5.6.2
#[gtest]
#[tokio::test]
async fn e011_01_05_check_that_one_can_update_an_existing_relationship_to_multiple_objects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = create_vehicle_brand_parked_speed_gps(&broker, &namespace, &VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS).await?;
    let id = namespace.urn("Vehicle", 1)?;
    let fragment = EntityFragment::builder()
        .relationship("isParked", parked_at_two_downtowns(&namespace)?)
        .build()?;
    update_attributes(&ld_json_body_client(&broker, &vocabulary)?, &id, &fragment).await?;
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
                "object": [
                    namespace.urn_text("OffStreetParking", "Downtown1"),
                    namespace.urn_text("OffStreetParking", "Downtown2"),
                ],
                "observedAt": "2017-07-29T12:00:04Z",
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
                "datasetId": namespace.urn_text("Relationship", "parked12345"),
            },
            "speed": speed_as_created(&namespace),
        }))
        .hiding_everywhere("@context")
    );
    delete_entities(&broker, &[&id]).await;
    Ok(())
}
