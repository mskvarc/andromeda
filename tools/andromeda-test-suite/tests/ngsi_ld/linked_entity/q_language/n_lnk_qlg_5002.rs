//! A hop takes every comparison operator after the closing brace.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: a q hop in the grammar's form R{attr}<operator>value is read for every comparison operator of clause 4.9
/// (!=, >, >=, <, <=, ~=, a range, a value list) and joins with | as well as with ;, and each selects the roots
/// whose Linked Entity satisfies it
/// Clause: 4.9, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_5002_a_hop_takes_every_comparison_operator_after_the_closing_brace() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let car_id = namespace.urn("Car", 1)?;
    let car_type = namespace.entity_type("Car")?;
    let car_2_id = namespace.urn("Car", 2)?;
    let car_3_id = namespace.urn("Car", 3)?;
    let car_4_id = namespace.urn("Car", 4)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("category", "commercial")
            .property("floors", 10)
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property("category", "residential")
            .property("floors", 3)
            .build()?,
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .property("category", "industrial")
            .property("floors", 20)
            .build()?,
        Entity::builder()
            .id(car_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_id.clone())
            .build()?,
        Entity::builder()
            .id(car_2_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_2_id.clone())
            .build()?,
        Entity::builder()
            .id(car_3_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_3_id.clone())
            .build()?,
        Entity::builder()
            .id(car_4_id.clone())
            .entity_type(car_type.clone())
            .property("color", "red")
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // !=
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("category").ne("commercial"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&car_2_id)));
    expect_that!(page.items(), contains(has_id(&car_3_id)));
    // >
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("floors").gt(10))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&car_3_id)));
    // >=
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("floors").ge(10))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), contains(has_id(&car_id)));
    expect_that!(page_3.items(), contains(has_id(&car_3_id)));
    // <
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("floors").lt(10))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(1)));
    expect_that!(page_4.items(), nth(0, has_id(&car_2_id)));
    // <=
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("floors").le(10))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(2)));
    expect_that!(page_5.items(), contains(has_id(&car_id)));
    expect_that!(page_5.items(), contains(has_id(&car_2_id)));
    // ~=
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("category").matches("\"^comm.*\""))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), nth(0, has_id(&car_id)));
    // range
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("floors").within(1, 5))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(1)));
    expect_that!(page_7.items(), nth(0, has_id(&car_2_id)));
    // value list
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("category").any_of(["commercial", "industrial"]))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_8.items(), len(eq(2)));
    expect_that!(page_8.items(), contains(has_id(&car_id)));
    expect_that!(page_8.items(), contains(has_id(&car_3_id)));
    // | joins a hop to a predicate on the root
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("category").eq("residential").or(q::attr("color").eq("red")))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_9.items(), len(eq(2)));
    expect_that!(page_9.items(), contains(has_id(&car_2_id)));
    expect_that!(page_9.items(), contains(has_id(&car_4_id)));
    Ok(())
}
