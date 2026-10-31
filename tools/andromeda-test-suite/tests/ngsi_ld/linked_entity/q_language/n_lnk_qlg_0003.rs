//! Hops nest across two relationships.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: q hops nest: R{R2{attr}}==value crosses two Relationships under `joinLevel=2` and still selects the
/// outermost root
/// Clause: 4.9, 4.5.23, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0003_hops_nest_across_two_relationships() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let district_north_id = namespace.urn("District", "north")?;
    let district_type = namespace.entity_type("District")?;
    let district_south_id = namespace.urn("District", "south")?;
    let building_north_id = namespace.urn("Building", "north")?;
    let building_type = namespace.entity_type("Building")?;
    let building_south_id = namespace.urn("Building", "south")?;
    let car_north_id = namespace.urn("Car", "north")?;
    let car_type = namespace.entity_type("Car")?;
    let car_south_id = namespace.urn("Car", "south")?;
    let entities = [
        Entity::builder()
            .id(district_north_id.clone())
            .entity_type(district_type.clone())
            .property("zone", "north")
            .build()?,
        Entity::builder()
            .id(district_south_id.clone())
            .entity_type(district_type.clone())
            .property("zone", "south")
            .build()?,
        Entity::builder()
            .id(building_north_id.clone())
            .entity_type(building_type.clone())
            .relationship("inDistrict", district_north_id.clone())
            .build()?,
        Entity::builder()
            .id(building_south_id.clone())
            .entity_type(building_type.clone())
            .relationship("inDistrict", district_south_id.clone())
            .build()?,
        Entity::builder()
            .id(car_north_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_north_id.clone())
            .build()?,
        Entity::builder()
            .id(car_south_id)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_south_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("inDistrict").linked("zone").eq("north"))
        .join(Join::Inline)
        .join_level(2.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&car_north_id)));
    Ok(())
}
