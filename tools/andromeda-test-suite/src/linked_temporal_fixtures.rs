//! The temporal entities the Linked Entity retrieval tests of Query Temporal Evolution read: two small graphs, each
//! recorded with Create Temporal Evolution before the query runs, and the read of a temporal resource with the
//! parameters (`join`, `joinLevel`, `pick`) no temporal query writes. Instances sit at 12:03 and 12:05 on 2020-08-01,
//! inside the 12:00-12:10 window the tests query.

use crate::{
    broker::BrokerClient,
    namespace::Namespace,
    outcome::Upserted,
    query_join::with_join,
    raw_call::{call, target},
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, Response};
use ngsi_ld::{
    http::{headers::Accept, operation::Operation},
    model::{Entity, EntityId, EntityTypeName, Input, NgsiLdMediaType, Property, Relationship, evolution::TemporalEntity},
    query::Join,
};

/// The instants a fixture records its instances at.
const FIRST: &str = "2020-08-01T12:03:00Z";

/// The second instant of a fixture.
const SECOND: &str = "2020-08-01T12:05:00Z";

/// The instant at which a vehicle parks outside the 12:00-12:10 window.
const OUTSIDE_THE_WINDOW: &str = "2020-09-01T13:00:00Z";

/// Vehicles `a`, `b` and `c`: `a` is parked in `b` at 12:03 and 12:05 and in `c` only on 2020-09-01, outside the window.
#[derive(Debug, Clone)]
pub struct ParkedVehicles {
    /// The vehicle that parks.
    pub a: EntityId,
    /// The vehicle `a` is parked in inside the window.
    pub b: EntityId,
    /// The vehicle `a` is parked in outside the window.
    pub c: EntityId,
    /// The type of the three.
    pub vehicle_type: EntityTypeName,
}

impl ParkedVehicles {
    /// The vehicles of the namespace.
    ///
    /// # Errors
    ///
    /// Returns the error of the namespace when an identifier or the type is not valid.
    pub fn new(namespace: &Namespace) -> Result<ParkedVehicles> {
        Ok(ParkedVehicles {
            a: namespace.urn("Vehicle", "a")?,
            b: namespace.urn("Vehicle", "b")?,
            c: namespace.urn("Vehicle", "c")?,
            vehicle_type: namespace.entity_type("Vehicle")?,
        })
    }

    /// Records vehicle `id` with the speeds `first` at 12:03 and `second` at 12:05.
    ///
    /// # Errors
    ///
    /// Returns the error of the build or of the broker.
    pub async fn record_speeds(&self, broker: &BrokerClient, id: &EntityId, first: i64, second: i64) -> Result<()> {
        let vehicle = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(self.vehicle_type.clone())
            .property("speed", Property::builder(first).observed_at(FIRST.parse()?))
            .property("speed", Property::builder(second).observed_at(SECOND.parse()?))
            .build()?;
        broker.temporal().create_or_update(&vehicle).send().await?.created()?;
        Ok(())
    }

    /// Records vehicle `a`: speeds 10 and 20, parked in `b` at 12:03 and 12:05 and in `c` outside the window.
    ///
    /// # Errors
    ///
    /// Returns the error of the build or of the broker.
    pub async fn record_a(&self, broker: &BrokerClient) -> Result<()> {
        let vehicle = TemporalEntity::builder()
            .id(self.a.clone())
            .entity_type(self.vehicle_type.clone())
            .property("speed", Property::builder(10).observed_at(FIRST.parse()?))
            .property("speed", Property::builder(20).observed_at(SECOND.parse()?))
            .relationship("isParkedIn", Relationship::builder(self.b.clone()).observed_at(FIRST.parse()?))
            .relationship("isParkedIn", Relationship::builder(self.b.clone()).observed_at(SECOND.parse()?))
            .relationship("isParkedIn", Relationship::builder(self.c.clone()).observed_at(OUTSIDE_THE_WINDOW.parse()?))
            .build()?;
        broker.temporal().create_or_update(&vehicle).send().await?.created()?;
        Ok(())
    }
}

/// A Building owned by a Person who works for a Company, in a City: `owner` and `tenant` name the Person, `locatedIn` the
/// City, and the Person's `employer` the Company.
#[derive(Debug, Clone)]
pub struct OwnedBuilding {
    /// The Company that employs the Person.
    pub company: EntityId,
    /// The Person.
    pub person: EntityId,
    /// The City.
    pub city: EntityId,
    /// The Building.
    pub building: EntityId,
    /// The type of the Company.
    pub company_type: EntityTypeName,
    /// The type of the Person.
    pub person_type: EntityTypeName,
    /// The type of the City.
    pub city_type: EntityTypeName,
    /// The type of the Building.
    pub building_type: EntityTypeName,
}

impl OwnedBuilding {
    /// The four entities of the namespace.
    ///
    /// # Errors
    ///
    /// Returns the error of the namespace when an identifier or a type is not valid.
    pub fn new(namespace: &Namespace) -> Result<OwnedBuilding> {
        Ok(OwnedBuilding {
            company: namespace.urn("Company", 1)?,
            person: namespace.urn("Person", 1)?,
            city: namespace.urn("City", 1)?,
            building: namespace.urn("Building", 1)?,
            company_type: namespace.entity_type("Company")?,
            person_type: namespace.entity_type("Person")?,
            city_type: namespace.entity_type("City")?,
            building_type: namespace.entity_type("Building")?,
        })
    }

    /// Records the Company, the Person, the City and the Building, in that order.
    ///
    /// # Errors
    ///
    /// Returns the error of the build or of the broker.
    pub async fn record(&self, broker: &BrokerClient) -> Result<()> {
        let company = TemporalEntity::builder()
            .id(self.company.clone())
            .entity_type(self.company_type.clone())
            .property("name", Property::builder("Acme").observed_at(FIRST.parse()?))
            .property("name", Property::builder("Acme").observed_at(SECOND.parse()?))
            .property("size", Property::builder(50).observed_at(FIRST.parse()?))
            .property("size", Property::builder(50).observed_at(SECOND.parse()?))
            .build()?;
        broker.temporal().create_or_update(&company).send().await?.created()?;
        let person = TemporalEntity::builder()
            .id(self.person.clone())
            .entity_type(self.person_type.clone())
            .property("name", Property::builder("Ada").observed_at(FIRST.parse()?))
            .property("name", Property::builder("Ada").observed_at(SECOND.parse()?))
            .property("email", Property::builder("ada@example.com").observed_at(FIRST.parse()?))
            .property("email", Property::builder("ada@example.com").observed_at(SECOND.parse()?))
            .relationship("employer", Relationship::builder(self.company.clone()).observed_at(FIRST.parse()?))
            .relationship("employer", Relationship::builder(self.company.clone()).observed_at(SECOND.parse()?))
            .build()?;
        broker.temporal().create_or_update(&person).send().await?.created()?;
        let city = TemporalEntity::builder()
            .id(self.city.clone())
            .entity_type(self.city_type.clone())
            .property("name", Property::builder("Paris").observed_at(FIRST.parse()?))
            .property("name", Property::builder("Paris").observed_at(SECOND.parse()?))
            .build()?;
        broker.temporal().create_or_update(&city).send().await?.created()?;
        let building = TemporalEntity::builder()
            .id(self.building.clone())
            .entity_type(self.building_type.clone())
            .property("name", Property::builder("Depot").observed_at(FIRST.parse()?))
            .property("name", Property::builder("Depot").observed_at(SECOND.parse()?))
            .relationship("owner", Relationship::builder(self.person.clone()).observed_at(FIRST.parse()?))
            .relationship("owner", Relationship::builder(self.person.clone()).observed_at(SECOND.parse()?))
            .relationship("tenant", Relationship::builder(self.person.clone()).observed_at(FIRST.parse()?))
            .relationship("tenant", Relationship::builder(self.person.clone()).observed_at(SECOND.parse()?))
            .relationship("locatedIn", Relationship::builder(self.city.clone()).observed_at(FIRST.parse()?))
            .relationship("locatedIn", Relationship::builder(self.city.clone()).observed_at(SECOND.parse()?))
            .build()?;
        broker.temporal().create_or_update(&building).send().await?.created()?;
        Ok(())
    }
}

/// The answer to `operation`, a typed temporal query or retrieval, sent with `join` and `joinLevel` appended to its query
/// string: the temporal operations write neither parameter.
///
/// # Errors
///
/// Returns the error of a request that cannot be built, sent or answered.
pub async fn send_joined<O: Operation + Sync>(broker: &BrokerClient, operation: &O, join: Join, level: u32) -> Result<Response<Bytes>> {
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let request = with_join(client.unchecked(operation)?, join, level)?;
    Ok(client.send_unchecked(request).await?)
}

/// The answer to `GET path` (relative to the API root) with `parameters` percent-encoded in order, asked for
/// `application/json`.
///
/// # Errors
///
/// Returns the error of the call when the request cannot be sent or answered.
pub async fn temporal_get(broker: &BrokerClient, path: &str, parameters: &[(&str, &str)]) -> Result<Response<Bytes>> {
    Ok(call(broker, Method::GET, &target(path, parameters), &[("Accept", "application/json")], None).await?)
}

/// The `links + 1` vehicles `urn:ngsi-ld:Vehicle:<token>-<n>` for `n` from 0 to `links`, each with a speed observed at
/// 12:03 and, below the last, an `isParkedIn` Relationship observed at 12:03 to the vehicle `n + 1`: a chain a join of
/// `links` levels from the first reaches whole.
///
/// # Errors
///
/// Returns the error of the namespace or the build when an identifier or an entity is refused.
pub fn vehicle_chain(namespace: &Namespace, links: usize) -> Result<Vec<Entity<Input>>> {
    let vehicle_type = namespace.entity_type("Vehicle")?;
    (0..=links)
        .map(|n| -> Result<Entity<Input>> {
            let builder = Entity::builder()
                .id(namespace.urn("Vehicle", n)?)
                .entity_type(vehicle_type.clone())
                .property("speed", Property::builder(10).observed_at(FIRST.parse()?));
            if n < links {
                Ok(builder
                    .relationship(
                        "isParkedIn",
                        Relationship::builder(namespace.urn("Vehicle", n + 1)?).observed_at(FIRST.parse()?),
                    )
                    .build()?)
            } else {
                Ok(builder.build()?)
            }
        })
        .collect()
}
