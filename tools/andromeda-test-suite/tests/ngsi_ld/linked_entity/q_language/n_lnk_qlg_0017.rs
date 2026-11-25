//! A list of one type before a further hop types the Entity the outer hop reaches.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: Query Entities with q isParkedIn{Building:owner{name}}=="ACME" selects the Car parked in a Building owned by
/// the Organization ACME and not the Car parked in a Warehouse owned by the same Organization
/// Clause: 4.9, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0017_a_list_of_one_type_before_a_further_hop_types_the_entity_the_outer_hop_reaches() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let organization_id = namespace.urn("Organization", "acme")?;
    let building_id = namespace.urn("Building", "owned")?;
    let building_type = namespace.entity_type("Building")?;
    let warehouse_id = namespace.urn("Warehouse", "owned")?;
    let car_type = namespace.entity_type("Car")?;
    let car_in_building_id = namespace.urn("Car", "in-building")?;
    let entities = [
        Entity::builder()
            .id(organization_id.clone())
            .entity_type(namespace.entity_type("Organization")?)
            .property("name", "ACME")
            .build()?,
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .relationship("owner", organization_id.clone())
            .build()?,
        Entity::builder()
            .id(warehouse_id.clone())
            .entity_type(namespace.entity_type("Warehouse")?)
            .relationship("owner", organization_id)
            .build()?,
        Entity::builder()
            .id(car_in_building_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_id)
            .build()?,
        Entity::builder()
            .id(namespace.urn("Car", "in-warehouse")?)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", warehouse_id)
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked_typed([&building_type], "owner").linked("name").eq("ACME"))
        .join(Join::Inline)
        .join_level(2.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&car_in_building_id)));
    Ok(())
}
