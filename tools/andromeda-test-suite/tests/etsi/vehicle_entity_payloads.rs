//! The Vehicle and parking entities and attribute fragments the entity tests load, their datasets, and the terms of the
//! `@context` those tests register.

use crate::geometry_payloads::point;
use andromeda_test_suite::namespace::Namespace;
use googletest::Result;
use ngsi_ld::{
    http::UncheckedDocument,
    model::{Attribute, DatasetId, Entity, EntityFragment, Input, Lossless, Patch, Property, Relationship, Value},
};
use std::fmt::Display;

/// The terms of the `@context` the replace cases register, as `vehicle_parked_speed_sources` names them.
pub(crate) const VEHICLE_PARKED_SPEED_SOURCE_TERMS: [&str; 5] = ["Vehicle", "isParked", "providedBy", "speed", "source"];

/// The `isParked` Relationship of the vehicle parked downtown: the `OffStreetParking` Downtown1, observed at
/// 2017-07-29T12:00:04Z, provided by Bob.
pub(crate) fn is_parked_downtown1(namespace: &Namespace) -> Result<Relationship<Input>> {
    Ok(Relationship::builder(namespace.urn("OffStreetParking", "Downtown1")?)
        .observed_at("2017-07-29T12:00:04Z".parse()?)
        .relationship("providedBy", namespace.urn("Person", "Bob")?)
        .build()?)
}

/// The `isParked` Relationship the replace cases write: the `OffStreetParking` Downtown2, observed at
/// 2017-07-29T15:00:04Z, provided by Bob.
pub(crate) fn is_parked_downtown2(namespace: &Namespace) -> Result<Relationship<Input>> {
    Ok(Relationship::builder(namespace.urn("OffStreetParking", "Downtown2")?)
        .observed_at("2017-07-29T15:00:04Z".parse()?)
        .relationship("providedBy", namespace.urn("Person", "Bob")?)
        .build()?)
}

/// The vehicle parked downtown, `urn:ngsi-ld:Vehicle:<ns>-<n>`: a Mercedes whose `isParked` Relationship is
/// [`is_parked_downtown1`].
pub(crate) fn vehicle_parked(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", suffix)?)
        .entity_type("Vehicle")
        .property("brandName", "Mercedes")
        .relationship("isParked", is_parked_downtown1(namespace)?)
        .build()?)
}

/// The `OffStreetParking` `urn:ngsi-ld:OffStreetParking:<ns>-<suffix>`: named Downtown One, with 121 available spots
/// observed at 2017-07-29T12:05:02Z (a reliability of 0.7, provided by the camera `urn:ngsi-ld:Camera:<ns>-C1`), 200
/// total spots, and located at the point -8.5, 41.2.
pub(crate) fn offstreet_parking(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    let available = Property::builder(121)
        .observed_at("2017-07-29T12:05:02Z".parse()?)
        .property("reliability", Property::builder(Value::try_from(0.7)?))
        .relationship("providedBy", Relationship::builder(namespace.urn("Camera", "C1")?));
    Ok(Entity::builder()
        .id(namespace.urn("OffStreetParking", suffix)?)
        .entity_type("OffStreetParking")
        .property("name", "Downtown One")
        .property("availableSpotNumber", available)
        .property("totalSpotNumber", Property::builder(200))
        .geo_property("location", point(-8.5, 41.2)?)
        .build()?)
}

/// The `datasetId` `urn:ngsi-ld:Property:<ns>-gpsBxyz123-speed` of the GPS `speed` instance of the replace cases.
pub(crate) fn gps_speed_dataset(namespace: &Namespace) -> Result<DatasetId> {
    Ok(DatasetId::from(namespace.urn("Property", "gpsBxyz123-speed")?.as_uri().clone()))
}

/// The `datasetId` `urn:ngsi-ld:Property:<ns>-speedometer` of the Speedometer `speed` instance.
pub(crate) fn speedometer_dataset(namespace: &Namespace) -> Result<DatasetId> {
    Ok(DatasetId::from(namespace.urn("Property", "speedometer")?.as_uri().clone()))
}

/// The `speed` of 56 in the dataset of [`speedometer_dataset`], with no source: the fragment a partial update writes
/// over that instance.
pub(crate) fn speedometer_speed_56_patch(namespace: &Namespace) -> Result<Attribute<Patch>> {
    Ok(Attribute::from(Property::builder(56).dataset_id(speedometer_dataset(namespace)?).build()?).into_patch())
}

/// The body of a partial update that changes only the `providedBy` Relationship to `urn:ngsi-ld:Person:<ns>-JohnDoe`.
/// It carries no payload of its own, which a typed attribute cannot hold.
pub(crate) fn provided_by_john_doe(namespace: &Namespace) -> Result<UncheckedDocument> {
    let provided_by = Attribute::from(Relationship::builder(namespace.urn("Person", "JohnDoe")?).build()?).render(Lossless::Normalized);
    Ok(UncheckedDocument::empty().set_member("providedBy", &provided_by))
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the replace cases: scope `/Madrid/Centro`, the `isParked` of
/// [`is_parked_downtown1`], and two `speed` instances, 55 from a Speedometer and 54.5 from a GPS in the dataset of
/// [`gps_speed_dataset`].
pub(crate) fn vehicle_parked_speed_sources(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .scope("/Madrid/Centro")
        .relationship("isParked", is_parked_downtown1(namespace)?)
        .property("speed", Property::builder(55).property("source", Property::builder("Speedometer")))
        .property(
            "speed",
            Property::builder(Value::try_from(54.5)?)
                .property("source", Property::builder("GPS"))
                .dataset_id(gps_speed_dataset(namespace)?),
        )
        .build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the delete cases: a `brandName` of Mercedes, an `isParked` Relationship
/// to the `OffStreetParking` Downtown1 observed on 2017-07-29 at 12:00:04 in UTC, provided by Bob, in the dataset
/// `urn:ngsi-ld:Relationship:<ns>-parked12345`, and two `speed` instances: 55 from a Speedometer as the default
/// instance, and 54.5 from a GPS in the dataset of [`gps_speed_dataset`].
pub(crate) fn vehicle_brand_parked_speed_gps(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(namespace.urn("OffStreetParking", "Downtown1")?)
                .observed_at("2017-07-29T12:00:04Z".parse()?)
                .relationship("providedBy", namespace.urn("Person", "Bob")?)
                .dataset_id(parked_dataset(namespace)?),
        )
        .property("speed", Property::builder(55).property("source", Property::builder("Speedometer")))
        .property(
            "speed",
            Property::builder(Value::try_from(54.5)?)
                .property("source", Property::builder("GPS"))
                .dataset_id(gps_speed_dataset(namespace)?),
        )
        .build()?)
}

/// The `datasetId` `urn:ngsi-ld:Dataset:<ns>-<name>`.
pub(crate) fn dataset(namespace: &Namespace, name: &str) -> Result<DatasetId> {
    Ok(DatasetId::from(namespace.urn("Dataset", name)?.as_uri().clone()))
}

/// The `datasetId` `urn:ngsi-ld:Relationship:<ns>-parked12345` of the `isParked` Relationship of the Vehicles that the
/// cases load.
pub(crate) fn parked_dataset(namespace: &Namespace) -> Result<DatasetId> {
    Ok(DatasetId::from(namespace.urn("Relationship", "parked12345")?.as_uri().clone()))
}

/// The terms of the `@context` the append cases on the Vehicle of [`vehicle_brand_parked_speed_sources`] register.
pub(crate) const VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS: [&str; 6] = ["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"];

/// The terms of the `@context` the append cases that send `attribute_to_be_added` register: the terms of
/// [`VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS`] and the appended `attribute_to_be_added`.
pub(crate) const VEHICLE_BRAND_PARKED_SPEED_SOURCE_ADDED_TERMS: [&str; 7] =
    ["Vehicle", "brandName", "isParked", "providedBy", "speed", "source", "attribute_to_be_added"];

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the append cases: a `brandName` of Mercedes, the `isParked` of
/// [`is_parked_downtown1`], and two `speed` instances, 55 from a Speedometer in the dataset
/// `urn:ngsi-ld:Property:<ns>-speedometer` and 54.5 from a GPS in the dataset of [`gps_speed_dataset`].
pub(crate) fn vehicle_brand_parked_speed_sources(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("brandName", "Mercedes")
        .relationship("isParked", is_parked_downtown1(namespace)?)
        .property(
            "speed",
            Property::builder(55)
                .property("source", Property::builder("Speedometer"))
                .dataset_id(speedometer_dataset(namespace)?),
        )
        .property(
            "speed",
            Property::builder(Value::try_from(54.5)?)
                .property("source", Property::builder("GPS"))
                .dataset_id(gps_speed_dataset(namespace)?),
        )
        .build()?)
}

/// The Speedometer `speed` of 56 in the dataset `urn:ngsi-ld:Property:<ns>-<dataset>`, which the append cases add.
pub(crate) fn speedometer_speed_56(namespace: &Namespace, dataset: &str) -> Result<Property<Input>> {
    Ok(Property::builder(56)
        .property("source", Property::builder("Speedometer"))
        .dataset_id(DatasetId::from(namespace.urn("Property", dataset)?.as_uri().clone()))
        .build()?)
}

/// The fragment the append cases send beside `attribute_to_be_added`: the Speedometer `speed` of 56 of
/// [`speedometer_speed_56`] in the dataset `dataset`, and an `attribute_to_be_added` of 23.
pub(crate) fn speed_and_added_attribute(namespace: &Namespace, dataset: &str) -> Result<EntityFragment> {
    Ok(EntityFragment::builder()
        .property("speed", speedometer_speed_56(namespace, dataset)?)
        .property("attribute_to_be_added", 23)
        .build()?)
}

/// The fragment that the update cases send: the Speedometer `speed` of 99 without a dataset, which replaces the
/// default instance of the setup.
pub(crate) fn speedometer_speed_99_fragment() -> Result<EntityFragment> {
    Ok(EntityFragment::builder()
        .property("speed", Property::builder(99).property("source", Property::builder("Speedometer")))
        .build()?)
}
