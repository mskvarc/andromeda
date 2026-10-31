//! The temporal evolutions of Vehicles and Buses the temporal tests load, the instances they send, and the terms of the
//! `@context` those tests register.

use andromeda_test_suite::namespace::Namespace;
use googletest::Result;
use ngsi_ld::model::{
    Attribute,
    AttributeName,
    DatasetId,
    DateTime,
    Input,
    LanguageMap,
    LanguageProperty,
    Property,
    Relationship,
    evolution::{TemporalAttributes, TemporalEntity},
};
use std::fmt::Display;

/// The terms of the `@context` the temporal Vehicle cases register, as `vehicle_speed_fuel_level` names them.
pub(crate) const VEHICLE_SPEED_FUEL_TERMS: [&str; 3] = ["Vehicle", "speed", "fuelLevel"];

/// The terms of the `@context` the add-attributes cases register: [`VEHICLE_SPEED_FUEL_TERMS`] and the attribute
/// `added_attribute` that they add.
pub(crate) const VEHICLE_SPEED_FUEL_ADDED_TERMS: [&str; 4] = ["Vehicle", "speed", "fuelLevel", "added_attribute"];

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-021-26` of the multi-valued relationship cases: an `isParkedIn` Relationship
/// whose objects are the `OffStreetParking` entities Downtown1 and Downtown2, observed on 2020-08-01 at 12:03 in UTC.
pub(crate) fn vehicle_parked_in_two(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", "021-26")?)
        .entity_type("Vehicle")
        .relationship(
            "isParkedIn",
            Relationship::builder(vec![
                namespace.urn("OffStreetParking", "Downtown1")?,
                namespace.urn("OffStreetParking", "Downtown2")?,
            ])
            .observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the temporal cases: a `speed` of 120, 80 and 100 observed at 12:03,
/// 12:05 and 12:07, and a `fuelLevel` of 67, 53 and 40 observed at 12:03, 13:05 and 14:07, all on 2020-08-01 in UTC.
pub(crate) fn vehicle_speed_fuel_level(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    vehicle_speed_fuel_level_on(namespace, 1, "2020-08-01")
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-<suffix>` of the temporal query cases: a `speed` of 120, 80 and 100 observed
/// at 12:03, 12:05 and 12:07, and a `fuelLevel` of 67, 53 and 40 observed at 12:03, 13:05 and 14:07, all on `day`
/// (`YYYY-MM-DD`) in UTC.
pub(crate) fn vehicle_speed_fuel_level_on(namespace: &Namespace, suffix: impl Display, day: &str) -> Result<TemporalEntity<Input>> {
    speed_fuel_entity(
        namespace,
        "Vehicle",
        suffix,
        day,
        [(120, "12:03:00Z"), (80, "12:05:00Z"), (100, "12:07:00Z")],
        [(67, "12:03:00Z"), (53, "13:05:00Z"), (40, "14:07:00Z")],
    )
}

/// The Bus `urn:ngsi-ld:Bus:<ns>-<suffix>` of the temporal query cases: a `speed` of 45, 25 and 67 observed at 12:03,
/// 12:05 and 12:07, and a `fuelLevel` of 210, 145 and 124 observed at 12:03, 13:05 and 14:07, all on 2020-08-01 in
/// UTC.
pub(crate) fn bus_speed_fuel_level(namespace: &Namespace, suffix: impl Display) -> Result<TemporalEntity<Input>> {
    speed_fuel_entity(
        namespace,
        "Bus",
        suffix,
        "2020-08-01",
        [(45, "12:03:00Z"), (25, "12:05:00Z"), (67, "12:07:00Z")],
        [(210, "12:03:00Z"), (145, "13:05:00Z"), (124, "14:07:00Z")],
    )
}

/// The entity of type `kind` with id `urn:ngsi-ld:<kind>:<ns>-<suffix>`, whose `speed` and `fuelLevel` instances are
/// each `(value, time)` on `day`, the time written as `hh:mm:ssZ`.
fn speed_fuel_entity(
    namespace: &Namespace,
    kind: &str,
    suffix: impl Display,
    day: &str,
    speeds: [(i32, &str); 3],
    fuel_levels: [(i32, &str); 3],
) -> Result<TemporalEntity<Input>> {
    let mut builder = TemporalEntity::builder().id(namespace.urn(kind, suffix)?).entity_type(kind);
    for (value, time) in speeds {
        builder = builder.property("speed", Property::builder(value).observed_at(format!("{day}T{time}").parse()?));
    }
    for (value, time) in fuel_levels {
        builder = builder.property("fuelLevel", Property::builder(value).observed_at(format!("{day}T{time}").parse()?));
    }
    Ok(builder.build()?)
}

/// The instant `minute` past 01:00 on 2020-01-01 in UTC.
fn instant_at(minute: u32) -> Result<DateTime> {
    Ok(format!("2020-01-01T01:{minute:02}:00Z").parse()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the cases that page through twenty instances: twenty speeds observed at
/// 01:01 to 01:20 and twenty fuel levels observed at 01:03 to 01:22, all on 2020-01-01 in UTC.
pub(crate) fn vehicle_twenty_instances(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    let mut builder = TemporalEntity::builder().id(namespace.urn("Vehicle", 1)?).entity_type("Vehicle");
    for minute in 1..=20 {
        builder = builder.property("speed", Property::builder(minute).observed_at(instant_at(minute)?));
    }
    for minute in 3..=22 {
        builder = builder.property("fuelLevel", Property::builder(minute - 2).observed_at(instant_at(minute)?));
    }
    Ok(builder.build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-<suffix>` of the query cases that page through five instances: a `speed` of
/// 85, 105, 120, 125 and 130 observed between 12:25 and 12:35, and a `fuelLevel` of 35, 31, 28, 24 and 19 observed
/// between 20:05 and 22:07, all on 2020-08-01 in UTC.
pub(crate) fn vehicle_five_instances(namespace: &Namespace, suffix: impl Display) -> Result<TemporalEntity<Input>> {
    let speeds = [
        (85, "2020-08-01T12:25:00Z"),
        (105, "2020-08-01T12:27:00Z"),
        (120, "2020-08-01T12:29:00Z"),
        (125, "2020-08-01T12:31:00Z"),
        (130, "2020-08-01T12:35:00Z"),
    ];
    let fuel_levels = [
        (35, "2020-08-01T20:05:00Z"),
        (31, "2020-08-01T20:07:00Z"),
        (28, "2020-08-01T21:03:00Z"),
        (24, "2020-08-01T21:05:00Z"),
        (19, "2020-08-01T22:07:00Z"),
    ];
    let mut builder = TemporalEntity::builder().id(namespace.urn("Vehicle", suffix)?).entity_type("Vehicle");
    for (value, observed_at) in speeds {
        builder = builder.property("speed", Property::builder(value).observed_at(observed_at.parse()?));
    }
    for (value, observed_at) in fuel_levels {
        builder = builder.property("fuelLevel", Property::builder(value).observed_at(observed_at.parse()?));
    }
    Ok(builder.build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the deleted-attribute cases: `isParkedIn` relationships to the
/// `OffStreetParking` Commerce and Graslin observed on 2024-12-01 and 2024-12-05, `name` language properties observed
/// on the same days, and `fuelLevel` properties of 67 and 53 observed on 2020-08-01 at 12:03 and 13:05, all in UTC.
pub(crate) fn vehicle_parking_name_fuel(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .relationship(
            "isParkedIn",
            Relationship::builder(namespace.urn("OffStreetParking", "Commerce")?).observed_at("2024-12-01T12:00:00Z".parse()?),
        )
        .relationship(
            "isParkedIn",
            Relationship::builder(namespace.urn("OffStreetParking", "Graslin")?).observed_at("2024-12-05T12:00:00Z".parse()?),
        )
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Ma voiture"), ("en", "My vehicle")])?).observed_at("2024-12-01T12:00:00Z".parse()?),
        )
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Ma jolie voiture"), ("en", "My pretty vehicle")])?)
                .observed_at("2024-12-05T12:00:00Z".parse()?),
        )
        .property("fuelLevel", Property::builder(67).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-08-01T13:05:00Z".parse()?))
        .build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the scope tests, without its `scope`, which the step that creates it
/// writes beside the rest: `fuelLevel` properties of 67 and 53 observed on 2020-08-01 at 12:03 and 13:05, in UTC.
pub(crate) fn vehicle_fuel_level(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("fuelLevel", Property::builder(67).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-08-01T13:05:00Z".parse()?))
        .build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the temporal provision cases: a `speed` of 120 and 80 observed at 12:03
/// and 12:05, and a `fuelLevel` of 67, 53 and 40 observed at 12:03, 13:05 and 14:07, the last one in the dataset
/// `urn:ngsi-ld:Vehicle:<ns>-12345-fuel`, all on 2020-09-01 in UTC.
pub(crate) fn vehicle_speed_fuel_level_with_dataset(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    let fuel_dataset = DatasetId::from(namespace.urn("Vehicle", "12345-fuel")?.as_uri().clone());
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40).observed_at("2020-09-01T14:07:00Z".parse()?).dataset_id(fuel_dataset),
        )
        .build()?)
}

/// The `speed` instance the partial-update cases send: 129 observed at 2020-09-01T12:03:00Z in UTC.
pub(crate) fn replacement_speed_instance() -> Result<Attribute<Input>> {
    Ok(Property::builder(129).observed_at("2020-09-01T12:03:00Z".parse()?).build()?.into())
}

/// The instances of the attribute `added_attribute` that the add-attributes cases send: 120, 80 and 100 observed at
/// 12:03, 12:05 and 12:07 on 2020-09-01 in UTC.
pub(crate) fn added_attribute_instances() -> Result<TemporalAttributes<Input>> {
    let mut attributes = TemporalAttributes::<Input>::new();
    for (value, observed_at) in [(120, "2020-09-01T12:03:00Z"), (80, "2020-09-01T12:05:00Z"), (100, "2020-09-01T12:07:00Z")] {
        let name: AttributeName = "added_attribute".parse()?;
        attributes.insert(name, Property::builder(value).observed_at(observed_at.parse()?).build()?.into())?;
    }
    Ok(attributes)
}
