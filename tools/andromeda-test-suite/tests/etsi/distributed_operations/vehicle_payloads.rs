//! The Vehicle entities the distributed-operations cases create on the broker, built with the client's builders and
//! namespaced for the test that creates them.

use andromeda_test_suite::namespace::Namespace;
use googletest::Result;
use ngsi_ld::model::{DatasetId, Entity, Input, Property, Relationship};

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-<n>` a BMW whose `isParked2` Relationship is the `OffStreetParking` Downtown2,
/// observed at 2017-07-29T12:00:04Z, provided by Alice.
pub(crate) fn vehicle_parked2(namespace: &Namespace, n: u32) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", n)?)
        .entity_type("Vehicle")
        .property("brandName", "BMW")
        .relationship("isParked2", parked_downtown2_by_alice(namespace)?)
        .build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-<n>` with a `speed` of 56 from the Speedometer in the dataset
/// `urn:ngsi-ld:Property:<ns>-speedometerA4567-speed2`, and the `isParked2` Relationship of [`vehicle_parked2`].
pub(crate) fn vehicle_speed_parked2(namespace: &Namespace, n: u32) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", n)?)
        .entity_type("Vehicle")
        .property(
            "speed",
            Property::builder(56)
                .property("source", "Speedometer")
                .dataset_id(namespace.urn_text("Property", "speedometerA4567-speed2").parse::<DatasetId>()?),
        )
        .relationship("isParked2", parked_downtown2_by_alice(namespace)?)
        .build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-<n>` with the `speed` of [`vehicle_speed_parked2`] and an `isParked2`
/// Relationship to the `OffStreetParking` Downtown2 observed at 2019-07-29T12:00:04Z and provided by Bob: the merge
/// the cases send.
pub(crate) fn vehicle_speed_parked2_by_bob(namespace: &Namespace, n: u32) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", n)?)
        .entity_type("Vehicle")
        .property(
            "speed",
            Property::builder(56)
                .property("source", "Speedometer")
                .dataset_id(namespace.urn_text("Property", "speedometerA4567-speed2").parse::<DatasetId>()?),
        )
        .relationship(
            "isParked2",
            Relationship::builder(namespace.urn("OffStreetParking", "Downtown2")?)
                .observed_at("2019-07-29T12:00:04Z".parse()?)
                .relationship("providedBy", namespace.urn("Person", "Bob")?)
                .build()?,
        )
        .build()?)
}

/// The Relationship to the `OffStreetParking` Downtown2, observed at 2017-07-29T12:00:04Z, provided by Alice.
fn parked_downtown2_by_alice(namespace: &Namespace) -> Result<Relationship<Input>> {
    Ok(Relationship::builder(namespace.urn("OffStreetParking", "Downtown2")?)
        .observed_at("2017-07-29T12:00:04Z".parse()?)
        .relationship("providedBy", namespace.urn("Person", "Alice")?)
        .build()?)
}
