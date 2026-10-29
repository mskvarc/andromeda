//! The `OffStreetParking` and `Vehicle` entities of the ETSI interoperability data files, as typed entities.
//!
//! The files carry fixed identifiers (`urn:ngsi-ld:OffStreetParking:1`) and types; the case's entities carry the
//! namespace's, so no two cases touch the same entity of a shared broker.

use andromeda_test_suite::namespace::{Namespace, NamespaceError};
use googletest::prelude::*;
use ngsi_ld::model::{
    DateTime,
    Entity,
    EntityId,
    EntityTypeName,
    GeoProperty,
    Input,
    Property,
    Relationship,
    Value,
    geometry::{Position, Shape},
};

/// The `name` attribute.
pub(crate) const NAME: &[&str] = &["name"];

/// The `availableSpotsNumber` attribute.
pub(crate) const AVAILABLE_SPOTS: &[&str] = &["availableSpotsNumber"];

/// The `totalSpotsNumber` attribute.
pub(crate) const TOTAL_SPOTS: &[&str] = &["totalSpotsNumber"];

/// The `availableSpotsNumber` and `totalSpotsNumber` attributes.
pub(crate) const SPOT_COUNTS: &[&str] = &["availableSpotsNumber", "totalSpotsNumber"];

/// The `location` attribute.
pub(crate) const LOCATION: &[&str] = &["location"];

/// The kind of entity a data file describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// An `OffStreetParking`.
    Parking,
    /// A `Vehicle`.
    Vehicle,
}

impl Kind {
    /// The type name the data files give the kind, before the namespace is added.
    pub(crate) const fn base(self) -> &'static str {
        match self {
            Kind::Parking => "OffStreetParking",
            Kind::Vehicle => "Vehicle",
        }
    }

    /// The identifier `urn:ngsi-ld:<kind>:<namespace>-<number>` of the kind's entity `number`.
    pub(crate) fn id(self, namespace: &Namespace, number: u32) -> Result<EntityId, NamespaceError> {
        namespace.urn(self.base(), number)
    }
}

/// The spot counts of a parking with the `observedAt` and `reliability` they carry.
struct Spots {
    /// The `availableSpotsNumber`.
    available: i64,
    /// The `observedAt` of the `availableSpotsNumber`.
    observed_at: &'static str,
    /// The `reliability` of the `availableSpotsNumber`.
    reliability: f64,
}

/// A point with the `observedAt` it may carry.
struct Place {
    /// The longitude.
    longitude: f64,
    /// The latitude.
    latitude: f64,
    /// The `observedAt` of the `location`, none when the file states none.
    observed_at: Option<&'static str>,
}

/// What a parking file holds.
struct ParkingFile {
    /// The number in the entity identifier.
    number: u32,
    /// The `name`.
    name: &'static str,
    /// The `availableSpotsNumber`, none when the file leaves it out.
    spots: Option<Spots>,
    /// The `totalSpotsNumber`, none when the file leaves it out.
    total: Option<i64>,
    /// The `location`, none when the file leaves it out.
    location: Option<Place>,
}

/// What a vehicle file holds.
struct VehicleFile {
    /// The number in the entity identifier, and in the identifier of the parking it is parked at.
    number: u32,
    /// The `brandName`.
    brand: &'static str,
    /// The `speed`.
    speed: i64,
    /// The `observedAt` of `isParked`.
    parked_observed_at: &'static str,
    /// The longitude and latitude of the `location`.
    position: (f64, f64),
}

/// A data file of `data/entities/interoperability`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sample {
    /// `full-version-of-OffStreetParking1`.
    Parking1Full,
    /// `full-version-of-OffStreetParking2`.
    Parking2Full,
    /// `offStreetParking1-without-location`.
    Parking1WithoutLocation,
    /// `offStreetParking2-without-location`.
    Parking2WithoutLocation,
    /// `offStreetParking1-with-location-and-name-only`.
    Parking1LocationAndName,
    /// `offStreetParking2-with-location-and-name-only`.
    Parking2LocationAndName,
    /// `offStreetParking2-without-location-and-totalSpotsNumber`.
    Parking2WithoutLocationAndTotal,
    /// `full-version-of-Vehicle1`.
    Vehicle1,
    /// `full-version-of-Vehicle2`.
    Vehicle2,
}

/// The file behind a sample.
enum File {
    /// A parking file.
    Parking(ParkingFile),
    /// A vehicle file.
    Vehicle(VehicleFile),
}

/// `full-version-of-OffStreetParking1`.
const PARKING_1_FULL: ParkingFile = ParkingFile {
    number: 1,
    name: "Downtown One",
    spots: Some(Spots {
        available: 121,
        observed_at: "2017-07-29T12:05:02Z",
        reliability: 0.7,
    }),
    total: Some(200),
    location: Some(Place {
        longitude: -8.5,
        latitude: 41.2,
        observed_at: None,
    }),
};

/// `full-version-of-OffStreetParking2`.
const PARKING_2_FULL: ParkingFile = ParkingFile {
    number: 2,
    name: "Downtown Two",
    spots: Some(Spots {
        available: 112,
        observed_at: "2017-07-29T12:05:02Z",
        reliability: 0.4,
    }),
    total: Some(150),
    location: Some(Place {
        longitude: -8.45,
        latitude: 41.2,
        observed_at: None,
    }),
};

/// `offStreetParking1-without-location`.
const PARKING_1_WITHOUT_LOCATION: ParkingFile = ParkingFile {
    number: 1,
    name: "Downtown One",
    spots: Some(Spots {
        available: 169,
        observed_at: "2017-07-29T12:10:02Z",
        reliability: 0.3,
    }),
    total: Some(200),
    location: None,
};

/// `offStreetParking2-without-location`.
const PARKING_2_WITHOUT_LOCATION: ParkingFile = ParkingFile {
    number: 2,
    name: "Downtown Two",
    spots: Some(Spots {
        available: 87,
        observed_at: "2017-07-29T12:10:02Z",
        reliability: 0.55,
    }),
    total: Some(150),
    location: None,
};

/// `offStreetParking1-with-location-and-name-only`.
const PARKING_1_LOCATION_AND_NAME: ParkingFile = ParkingFile {
    number: 1,
    name: "Downtown One",
    spots: None,
    total: None,
    location: Some(Place {
        longitude: -8.9,
        latitude: 42.2,
        observed_at: Some("2021-04-04T11:45:00Z"),
    }),
};

/// `offStreetParking2-with-location-and-name-only`.
const PARKING_2_LOCATION_AND_NAME: ParkingFile = ParkingFile {
    number: 2,
    name: "Downtown Two",
    spots: None,
    total: None,
    location: Some(Place {
        longitude: -8.46,
        latitude: 41.25,
        observed_at: Some("2021-04-04T11:46:00Z"),
    }),
};

/// `offStreetParking2-without-location-and-totalSpotsNumber`.
const PARKING_2_WITHOUT_LOCATION_AND_TOTAL: ParkingFile = ParkingFile {
    number: 2,
    name: "Downtown Two",
    spots: Some(Spots {
        available: 92,
        observed_at: "2017-07-30T12:10:02Z",
        reliability: 0.56,
    }),
    total: None,
    location: None,
};

/// `full-version-of-Vehicle1`.
const VEHICLE_1: VehicleFile = VehicleFile {
    number: 1,
    brand: "Brand1",
    speed: 0,
    parked_observed_at: "2017-07-29T12:00:04Z",
    position: (-8.45, 41.2),
};

/// `full-version-of-Vehicle2`.
const VEHICLE_2: VehicleFile = VehicleFile {
    number: 2,
    brand: "Brand2",
    speed: 5,
    parked_observed_at: "2017-07-30T12:00:04Z",
    position: (-9.45, 40.2),
};

impl Sample {
    /// What the file holds.
    const fn file(self) -> File {
        match self {
            Sample::Parking1Full => File::Parking(PARKING_1_FULL),
            Sample::Parking2Full => File::Parking(PARKING_2_FULL),
            Sample::Parking1WithoutLocation => File::Parking(PARKING_1_WITHOUT_LOCATION),
            Sample::Parking2WithoutLocation => File::Parking(PARKING_2_WITHOUT_LOCATION),
            Sample::Parking1LocationAndName => File::Parking(PARKING_1_LOCATION_AND_NAME),
            Sample::Parking2LocationAndName => File::Parking(PARKING_2_LOCATION_AND_NAME),
            Sample::Parking2WithoutLocationAndTotal => File::Parking(PARKING_2_WITHOUT_LOCATION_AND_TOTAL),
            Sample::Vehicle1 => File::Vehicle(VEHICLE_1),
            Sample::Vehicle2 => File::Vehicle(VEHICLE_2),
        }
    }

    /// The entity the file holds, with the identifier and type of `namespace`.
    pub(crate) fn entity(self, namespace: &Namespace) -> Result<Entity<Input>> {
        match self.file() {
            File::Parking(parking) => parking_entity(namespace, &parking),
            File::Vehicle(vehicle) => vehicle_entity(namespace, &vehicle),
        }
    }
}

/// The type of the kind in `namespace`.
fn type_of(namespace: &Namespace, kind: Kind) -> Result<EntityTypeName> {
    Ok(namespace.entity_type(kind.base())?)
}

/// The point of a position.
fn point(longitude: f64, latitude: f64) -> Result<Shape> {
    Ok(Shape::Point(Position::try_new(longitude, latitude)?))
}

/// The parking `parking` as an entity of `namespace`.
fn parking_entity(namespace: &Namespace, parking: &ParkingFile) -> Result<Entity<Input>> {
    let mut builder = Entity::builder()
        .id(Kind::Parking.id(namespace, parking.number)?)
        .entity_type(type_of(namespace, Kind::Parking)?)
        .property("name", parking.name);
    if let Some(spots) = &parking.spots {
        let available = Property::builder(spots.available)
            .observed_at(spots.observed_at.parse::<DateTime>()?)
            .property("reliability", Value::try_from(spots.reliability)?);
        builder = builder.property("availableSpotsNumber", available);
    }
    if let Some(total) = parking.total {
        builder = builder.property("totalSpotsNumber", total);
    }
    if let Some(place) = &parking.location {
        let shape = point(place.longitude, place.latitude)?;
        let observed_at = place.observed_at.map(str::parse::<DateTime>).transpose()?;
        let location = GeoProperty::builder(shape).maybe_observed_at(observed_at);
        builder = builder.geo_property("location", location);
    }
    Ok(builder.build()?)
}

/// The vehicle `vehicle` as an entity of `namespace`, parked at the parking of the same number.
fn vehicle_entity(namespace: &Namespace, vehicle: &VehicleFile) -> Result<Entity<Input>> {
    let speed = Property::builder(vehicle.speed).property("source", Value::from("Speedometer"));
    let parked = Relationship::builder(Kind::Parking.id(namespace, vehicle.number)?).observed_at(vehicle.parked_observed_at.parse::<DateTime>()?);
    Ok(Entity::builder()
        .id(Kind::Vehicle.id(namespace, vehicle.number)?)
        .entity_type(type_of(namespace, Kind::Vehicle)?)
        .property("brandName", vehicle.brand)
        .property("speed", speed)
        .relationship("isParked", parked)
        .geo_property("location", GeoProperty::builder(point(vehicle.position.0, vehicle.position.1)?))
        .build()?)
}

#[cfg(test)]
mod tests {
    use crate::parking::Sample;
    use andromeda_test_suite::namespace::Namespace;
    use googletest::prelude::*;
    use ngsi_ld::model::Representation;
    use serde_json::{Value, json};

    fn rendered(sample: Sample, namespace: &Namespace) -> Result<Value> {
        Ok(serde_json::from_str(&sample.entity(namespace)?.render(Representation::Normalized).to_string())?)
    }

    #[gtest]
    fn the_full_first_parking_carries_the_values_of_its_file() -> Result<()> {
        let namespace = Namespace::fresh();
        let body = rendered(Sample::Parking1Full, &namespace)?;
        expect_that!(body.get("id"), some(eq(&json!(namespace.urn_text("OffStreetParking", 1)))));
        expect_that!(body.get("type"), some(eq(&json!(namespace.suffixed("OffStreetParking")))));
        expect_that!(body.get("name"), some(eq(&json!({"type": "Property", "value": "Downtown One"}))));
        expect_that!(
            body.get("availableSpotsNumber"),
            some(eq(&json!({
                "type": "Property",
                "value": 121,
                "observedAt": "2017-07-29T12:05:02Z",
                "reliability": {"type": "Property", "value": 0.7},
            })))
        );
        expect_that!(body.get("totalSpotsNumber"), some(eq(&json!({"type": "Property", "value": 200}))));
        expect_that!(
            body.get("location"),
            some(eq(&json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}})))
        );
        Ok(())
    }

    #[gtest]
    fn the_location_and_name_file_carries_the_observation_time_of_its_location() -> Result<()> {
        let namespace = Namespace::fresh();
        let body = rendered(Sample::Parking2LocationAndName, &namespace)?;
        expect_that!(
            body.get("location"),
            some(eq(&json!({
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [-8.46, 41.25]},
                "observedAt": "2021-04-04T11:46:00Z",
            })))
        );
        expect_that!(body.get("availableSpotsNumber"), none());
        expect_that!(body.get("totalSpotsNumber"), none());
        Ok(())
    }

    #[gtest]
    fn the_file_without_location_and_total_keeps_the_counts_of_its_name() -> Result<()> {
        let namespace = Namespace::fresh();
        let body = rendered(Sample::Parking2WithoutLocationAndTotal, &namespace)?;
        expect_that!(body.get("location"), none());
        expect_that!(body.get("totalSpotsNumber"), none());
        expect_that!(
            body.get("availableSpotsNumber"),
            some(eq(&json!({
                "type": "Property",
                "value": 92,
                "observedAt": "2017-07-30T12:10:02Z",
                "reliability": {"type": "Property", "value": 0.56},
            })))
        );
        Ok(())
    }

    #[gtest]
    fn the_second_vehicle_is_parked_at_the_second_parking() -> Result<()> {
        let namespace = Namespace::fresh();
        let body = rendered(Sample::Vehicle2, &namespace)?;
        expect_that!(
            body.get("isParked"),
            some(eq(&json!({
                "type": "Relationship",
                "object": namespace.urn_text("OffStreetParking", 2),
                "observedAt": "2017-07-30T12:00:04Z",
            })))
        );
        expect_that!(
            body.get("speed"),
            some(eq(
                &json!({"type": "Property", "value": 5, "source": {"type": "Property", "value": "Speedometer"}})
            ))
        );
        Ok(())
    }
}
