//! A q hop with a type list selects the same Entities in Query Entities and in a Subscription.

use andromeda_q_corpus::corpus::Traversal;
use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    q_parity::{QCase, QRequest, expect_query_and_subscription_alike},
};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, Input};

/// The request a term with one hop is asked with.
const ONE_HOP: QRequest<'static> = QRequest {
    expand_values: &[],
    json_keys: &[],
    traversal: Traversal::OneHop,
};

/// The request a term whose hop holds a further hop is asked with.
const TWO_HOPS: QRequest<'static> = QRequest {
    expand_values: &[],
    json_keys: &[],
    traversal: Traversal::TwoHops,
};

/// Pins: over the weather stations of clause 4.9 EXAMPLE 14, one sensing through a Device and one through a
/// Thermometer, q sensor{Device:humidity}==40 selects the first in Query Entities and in a Subscription alike; over
/// Cars parked in a Building, a Warehouse and an Entity typed both, each holding category commercial and owned by ACME,
/// q isParkedIn{Building,Garage:category}=="commercial", q isParkedIn{Building:owner{name}}=="ACME" and q
/// isParkedIn{Building:category}=="commercial" select the same Cars in both
/// Clause: 4.9, 5.7.2.4, 5.8.6
#[gtest]
#[tokio::test]
async fn n_lnk_ntf_0006_a_q_hop_with_a_type_list_selects_the_same_entities_in_a_query_and_a_subscription() -> Result<()> {
    let broker = broker::front()?;
    let stations = Namespace::fresh();
    let station_type = stations.entity_type("WeatherStation")?;
    let device_type = stations.entity_type("Device")?;
    let device_id = stations.urn("Device", 345)?;
    let thermometer_id = stations.urn("Thermometer", 345)?;
    let through_device = stations.urn("WeatherStation", "device")?;
    let station_entities: Vec<Entity<Input>> = vec![
        Entity::builder()
            .id(device_id.clone())
            .entity_type(device_type.clone())
            .property("humidity", 40)
            .build()?,
        Entity::builder()
            .id(thermometer_id.clone())
            .entity_type(stations.entity_type("Thermometer")?)
            .property("humidity", 40)
            .build()?,
        Entity::builder()
            .id(through_device.clone())
            .entity_type(station_type.clone())
            .relationship("sensor", device_id)
            .build()?,
        Entity::builder()
            .id(stations.urn("WeatherStation", "thermometer")?)
            .entity_type(station_type.clone())
            .relationship("sensor", thermometer_id)
            .build()?,
    ];
    let device_q = format!("sensor{{{device_type}:humidity}}==40");
    let station_cases = [QCase {
        q: &device_q,
        selects: &[&through_device],
        request: ONE_HOP,
    }];
    expect_query_and_subscription_alike(&broker, &stations, &station_type, &station_entities, &station_cases).await?;

    let cars = Namespace::fresh();
    let car_type = cars.entity_type("Car")?;
    let building_type = cars.entity_type("Building")?;
    let warehouse_type = cars.entity_type("Warehouse")?;
    let organization_id = cars.urn("Organization", "acme")?;
    let building_id = cars.urn("Building", "owned")?;
    let warehouse_id = cars.urn("Warehouse", "owned")?;
    let depot_id = cars.urn("Depot", "both")?;
    let in_building = cars.urn("Car", "in-building")?;
    let in_depot = cars.urn("Car", "in-depot")?;
    let parking = |id, types: Vec<_>| -> Result<Entity<Input>> {
        Ok(Entity::builder()
            .id(id)
            .entity_type(types)
            .property("category", "commercial")
            .relationship("owner", organization_id.clone())
            .build()?)
    };
    let car = |id, parked_in| -> Result<Entity<Input>> {
        Ok(Entity::builder()
            .id(id)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", parked_in)
            .build()?)
    };
    let car_entities = vec![
        Entity::builder()
            .id(organization_id.clone())
            .entity_type(cars.entity_type("Organization")?)
            .property("name", "ACME")
            .build()?,
        parking(building_id.clone(), vec![building_type.clone()])?,
        parking(warehouse_id.clone(), vec![warehouse_type.clone()])?,
        parking(depot_id.clone(), vec![building_type.clone(), warehouse_type])?,
        car(in_building.clone(), building_id)?,
        car(cars.urn("Car", "in-warehouse")?, warehouse_id)?,
        car(in_depot.clone(), depot_id)?,
    ];
    let garage_type = cars.entity_type("Garage")?;
    let listed_q = format!("isParkedIn{{{building_type},{garage_type}:category}}==\"commercial\"");
    let owner_q = format!("isParkedIn{{{building_type}:owner{{name}}}}==\"ACME\"");
    let single_q = format!("isParkedIn{{{building_type}:category}}==\"commercial\"");
    let car_cases = [
        QCase {
            q: &listed_q,
            selects: &[&in_building, &in_depot],
            request: ONE_HOP,
        },
        QCase {
            q: &owner_q,
            selects: &[&in_building, &in_depot],
            request: TWO_HOPS,
        },
        QCase {
            q: &single_q,
            selects: &[&in_building, &in_depot],
            request: ONE_HOP,
        },
    ];
    expect_query_and_subscription_alike(&broker, &cars, &car_type, &car_entities, &car_cases).await
}
