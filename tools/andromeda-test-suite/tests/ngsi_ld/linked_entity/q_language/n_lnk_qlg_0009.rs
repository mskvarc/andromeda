//! A hop chain deeper than join level is bad request data, with or without type lists.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: a q hop chain deeper than `joinLevel` is a too deep query and `BadRequestData`, and the same chain is
/// admitted once `joinLevel` reaches its depth, for isParkedIn{inDistrict{zone}}=="north" and for the chain with a
/// type list on each hop, isParkedIn{Building:inDistrict{District:zone}}=="north", alike
/// Clause: 5.7.2.4, 4.5.23, 4.9
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0009_a_hop_chain_deeper_than_join_level_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let district_north_id = namespace.urn("District", "north")?;
    let district_type = namespace.entity_type("District")?;
    let building_north_id = namespace.urn("Building", "north")?;
    let building_type = namespace.entity_type("Building")?;
    let car_north_id = namespace.urn("Car", "north")?;
    let car_type = namespace.entity_type("Car")?;
    let entities = [
        Entity::builder()
            .id(district_north_id.clone())
            .entity_type(district_type.clone())
            .property("zone", "north")
            .build()?,
        Entity::builder()
            .id(building_north_id.clone())
            .entity_type(building_type.clone())
            .relationship("inDistrict", district_north_id.clone())
            .build()?,
        Entity::builder()
            .id(car_north_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_north_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let chains = [
        ("untyped", q::attr("isParkedIn").linked("inDistrict").linked("zone").eq("north")),
        (
            "typed",
            q::attr("isParkedIn")
                .linked_typed([&building_type], "inDistrict")
                .linked_typed([&district_type], "zone")
                .eq("north"),
        ),
    ];
    for (spelling, chain) in chains {
        let answer = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&car_type)
            .q(chain.clone())
            .join(Join::Inline)
            .join_level(1.try_into()?)
            .send()
            .await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData), "{spelling}");
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&car_type)
            .q(chain)
            .join(Join::Inline)
            .join_level(2.try_into()?)
            .send()
            .await?;
        expect_that!(page.items(), len(eq(1)), "{spelling}");
        expect_that!(page.items(), nth(0, has_id(&car_north_id)), "{spelling}");
    }
    Ok(())
}
