//! A hop selects the root whose linked entity matches.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, type_matcher::has_type};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: q R{attr}==value compares an attribute of the entity the Relationship names and selects the root that
/// holds the Relationship, not the entity compared; joined with ; to a predicate on the root, only a root meeting
/// both is selected
/// Clause: 4.9, 4.5.23, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0001_a_hop_selects_the_root_whose_linked_entity_matches() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_commercial_id = namespace.urn("Building", "commercial")?;
    let building_type = namespace.entity_type("Building")?;
    let building_industrial_id = namespace.urn("Building", "industrial")?;
    let car_c1_id = namespace.urn("Car", "c1")?;
    let car_type = namespace.entity_type("Car")?;
    let car_c2_id = namespace.urn("Car", "c2")?;
    let car_both_id = namespace.urn("Car", "both")?;
    let car_slow_id = namespace.urn("Car", "slow")?;
    let car_wrong_building_id = namespace.urn("Car", "wrong-building")?;
    let entities = [
        Entity::builder()
            .id(building_commercial_id.clone())
            .entity_type(building_type.clone())
            .property("category", "commercial")
            .build()?,
        Entity::builder()
            .id(building_industrial_id.clone())
            .entity_type(building_type.clone())
            .property("category", "industrial")
            .build()?,
        Entity::builder()
            .id(car_c1_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_commercial_id.clone())
            .build()?,
        Entity::builder()
            .id(car_c2_id)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_industrial_id.clone())
            .build()?,
        Entity::builder()
            .id(car_both_id.clone())
            .entity_type(car_type.clone())
            .property("speed", 80)
            .relationship("isParkedIn", building_commercial_id.clone())
            .build()?,
        Entity::builder()
            .id(car_slow_id.clone())
            .entity_type(car_type.clone())
            .property("speed", 20)
            .relationship("isParkedIn", building_commercial_id.clone())
            .build()?,
        Entity::builder()
            .id(car_wrong_building_id)
            .entity_type(car_type.clone())
            .property("speed", 90)
            .relationship("isParkedIn", building_industrial_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // every Car parked in the commercial Building, whatever else it holds; no Building is answered
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("category").eq("commercial"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), contains(all!(has_id(&car_c1_id), has_type(&car_type))));
    expect_that!(page.items(), contains(all!(has_id(&car_both_id), has_type(&car_type))));
    expect_that!(page.items(), contains(all!(has_id(&car_slow_id), has_type(&car_type))));
    // neither the local predicate alone nor the hop alone admits a root
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("speed").gt(50).and(q::attr("isParkedIn").linked("category").eq("commercial")))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&car_both_id)));
    Ok(())
}
