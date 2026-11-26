//! A list of two types before a further hop admits a linked Entity of either type.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: over Cars parked in a Building, a Garage and a Warehouse, each owned by the Organization ACME, Query Entities
/// with q isParkedIn{Building,Garage:owner{name}}=="ACME" selects the Cars parked in the Building and in the Garage
/// and not the one parked in the Warehouse
/// Clause: 4.9, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0021_a_list_of_two_types_before_a_further_hop_admits_a_linked_entity_of_either() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let organization_id = namespace.urn("Organization", "acme")?;
    let building_type = namespace.entity_type("Building")?;
    let garage_type = namespace.entity_type("Garage")?;
    let car_type = namespace.entity_type("Car")?;
    let in_building = namespace.urn("Car", "in-building")?;
    let in_garage = namespace.urn("Car", "in-garage")?;
    let mut entities = vec![
        Entity::builder()
            .id(organization_id.clone())
            .entity_type(namespace.entity_type("Organization")?)
            .property("name", "ACME")
            .build()?,
    ];
    for (place, place_type, car_id) in [
        ("building", building_type.clone(), in_building.clone()),
        ("garage", garage_type.clone(), in_garage.clone()),
        ("warehouse", namespace.entity_type("Warehouse")?, namespace.urn("Car", "in-warehouse")?),
    ] {
        let place_id = namespace.urn("Place", place)?;
        entities.push(
            Entity::builder()
                .id(place_id.clone())
                .entity_type(place_type)
                .relationship("owner", organization_id.clone())
                .build()?,
        );
        entities.push(
            Entity::builder()
                .id(car_id)
                .entity_type(car_type.clone())
                .relationship("isParkedIn", place_id)
                .build()?,
        );
    }
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn")
            .linked_typed([&building_type, &garage_type], "owner")
            .linked("name")
            .eq("ACME"))
        .join(Join::Inline)
        .join_level(2.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), unordered_elements_are![has_id(&in_building), has_id(&in_garage)]);
    Ok(())
}
