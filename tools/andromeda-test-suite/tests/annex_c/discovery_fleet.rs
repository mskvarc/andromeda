//! The four Entities the discovery examples of Annex C.5.7 to C.5.12 describe: two Vehicles, the parking they are
//! parked at, and a parking spot whose type and attribute no supplied `@context` names.

use crate::{
    served_context::{ServedContexts, linked},
    vehicle_example::aggregated_context,
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{
    Entity,
    Input,
    geometry::{Position, Shape},
};

/// A Vehicle of the fleet.
fn vehicle(namespace: &Namespace, suffix: &str, brand: &str, speed: i64, position: (f64, f64)) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", suffix)?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("brandName", brand)
        .property("speed", speed)
        .relationship("isParked", namespace.urn("OffStreetParking", "Downtown1")?)
        .geo_property("location", Shape::Point(Position::try_new(position.0, position.1)?))
        .build()?)
}

/// The fleet: the Vehicles A4567 and B9211, the parking Downtown1 and the parking spot S1.
fn fleet(namespace: &Namespace) -> Result<Vec<Entity<Input>>> {
    let parking = Entity::builder()
        .id(namespace.urn("OffStreetParking", "Downtown1")?)
        .entity_type(namespace.entity_type("OffStreetParking")?)
        .property("availableSpotNumber", 121)
        .property("totalSpotNumber", 200)
        .relationship("isNextToBuilding", namespace.urn("Building", "B1")?)
        .geo_property("location", Shape::Point(Position::try_new(-8.5, 41.2)?))
        .build()?;
    let spot = Entity::builder()
        .id(namespace.urn("ParkingSpot", "S1")?)
        .entity_type(format!("http://example.org/parking/{}", namespace.suffixed("ParkingSpot")).as_str())
        .property("http://example.org/parking/status", "free")
        .geo_property("location", Shape::Point(Position::try_new(-8.5, 41.2)?))
        .build()?;
    Ok(vec![
        vehicle(namespace, "A4567", "Mercedes", 55, (-8.5, 41.2))?,
        vehicle(namespace, "B9211", "Volvo", 60, (-8.6, 41.3))?,
        parking,
        spot,
    ])
}

/// Brings up the namespace's tenant, serves the aggregated `@context`, and creates the fleet there in one batch
/// (201); answers the tenant's client sending the aggregated `@context`, and the server of it.
pub(crate) async fn fleet_tenant(namespace: &Namespace) -> Result<(ServedContexts, BrokerClient)> {
    let tenant = broker::private_tenant(&broker::front()?, namespace).await?;
    let served = ServedContexts::start(namespace).await?;
    let client = linked(&tenant, &aggregated_context(&served, namespace).await?)?;
    let created = client.with_responses().entities().batch_create(&fleet(namespace)?).send().await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    Ok((served, client))
}
