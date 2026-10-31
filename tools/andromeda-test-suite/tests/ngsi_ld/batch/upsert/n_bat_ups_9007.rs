//! A feed with kilobyte references is stored whole.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    body_json::json_count,
    broker,
    entity_matcher::has_attribute,
    exact_length,
    namespace::Namespace,
    outcome::UpsertCreated,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ListRelationship, NgsiLdMediaType},
};

/// Pins: Batch Entity Upsert of a trip update whose `ListRelationship` names four URNs of 1,047 to 5,763 octets, an
/// alert naming two of 1,500 and 4,200, and a vehicle beside them answers 201, and each reads back unchanged
/// Clause: 5.6.8, 4.6.2, Table 5.2.37-1
#[gtest]
#[tokio::test]
async fn n_bat_ups_9007_a_feed_with_kilobyte_references_is_stored_whole() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let trip_id = namespace.urn("GtfsRtTripUpdate", "trip-1")?;
    let trip_type = namespace.entity_type("GtfsRtTripUpdate")?;
    let alert_id = namespace.urn("GtfsRtAlert", "alert-1")?;
    let alert_type = namespace.entity_type("GtfsRtAlert")?;
    let vehicle_id = namespace.urn("GtfsRtVehiclePosition", "vehicle-1")?;
    let vehicle_type = namespace.entity_type("GtfsRtVehiclePosition")?;
    let stop_time_updates = [1013, 2277, 3954, 5729]
        .into_iter()
        .map(|length| exact_length::urn(&namespace, "GtfsRtStopTimeUpdate", length))
        .collect::<Result<Vec<_>, _>>()?;
    let informed_entities = [1466, 4166]
        .into_iter()
        .map(|length| exact_length::urn(&namespace, "GtfsRtInformedEntity", length))
        .collect::<Result<Vec<_>, _>>()?;
    let entities = [
        Entity::builder()
            .id(trip_id.clone())
            .entity_type(trip_type)
            .list_relationship("stopTimeUpdates", ListRelationship::builder(stop_time_updates.clone()))
            .build()?,
        Entity::builder()
            .id(alert_id.clone())
            .entity_type(alert_type)
            .list_relationship("informedEntities", ListRelationship::builder(informed_entities.clone()))
            .build()?,
        Entity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type)
            .property("bearing", 90)
            .build()?,
    ];
    let created = broker.entities().batch_upsert(&entities).send().await?.created_entities()?;
    expect_that!(created, len(eq(3)));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&trip_id)
        .send()
        .await?;
    let trip = received.outcome.body();
    expect_that!(trip, has_attribute("stopTimeUpdates", any_attribute().list_length(4)));
    // The model reads an `objectList` item written as a bare URI or as an object alike, so the item spelling is read as
    // received.
    expect_that!(&received.body, json_count("$.stopTimeUpdates.objectList", 4));
    for (index, reference) in stop_time_updates.iter().enumerate() {
        expect_that!(trip, has_attribute("stopTimeUpdates", any_attribute().list_object(index, reference.clone())));
        expect_that!(&received.body, json_count(&format!("$.stopTimeUpdates.objectList[{index}]"), 1));
    }
    let received_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&alert_id)
        .send()
        .await?;
    let alert = received_2.outcome.body();
    expect_that!(alert, has_attribute("informedEntities", any_attribute().list_length(2)));
    expect_that!(&received_2.body, json_count("$.informedEntities.objectList", 2));
    for (index, reference) in informed_entities.iter().enumerate() {
        expect_that!(alert, has_attribute("informedEntities", any_attribute().list_object(index, reference.clone())));
        expect_that!(&received_2.body, json_count(&format!("$.informedEntities.objectList[{index}]"), 1));
    }
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let vehicle = retrieved.body();
    expect_that!(vehicle, has_attribute("bearing", any_attribute().value(90)));
    Ok(())
}
