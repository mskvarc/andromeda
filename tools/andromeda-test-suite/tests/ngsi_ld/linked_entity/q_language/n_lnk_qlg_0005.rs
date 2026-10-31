//! Count covers every root a hop selects not the page.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: with a q hop, count=true reports every root the hop selects in NGSILD-Results-Count while limit bounds the
/// page, so the hop is applied before paging
/// Clause: 4.9, 4.5.23, 5.5.9, 6.3.13
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0005_count_covers_every_root_a_hop_selects_not_the_page() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let garage_commercial_id = namespace.urn("Garage", "commercial")?;
    let garage_type = namespace.entity_type("Garage")?;
    let garage_industrial_id = namespace.urn("Garage", "industrial")?;
    let car_m1_id = namespace.urn("Car", "m1")?;
    let car_type = namespace.entity_type("Car")?;
    let car_m2_id = namespace.urn("Car", "m2")?;
    let car_m3_id = namespace.urn("Car", "m3")?;
    let car_x1_id = namespace.urn("Car", "x1")?;
    let car_x2_id = namespace.urn("Car", "x2")?;
    let entities = [
        Entity::builder()
            .id(garage_commercial_id.clone())
            .entity_type(garage_type.clone())
            .property("category", "commercial")
            .build()?,
        Entity::builder()
            .id(garage_industrial_id.clone())
            .entity_type(garage_type.clone())
            .property("category", "industrial")
            .build()?,
        Entity::builder()
            .id(car_m1_id)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", garage_commercial_id.clone())
            .build()?,
        Entity::builder()
            .id(car_m2_id)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", garage_commercial_id.clone())
            .build()?,
        Entity::builder()
            .id(car_m3_id)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", garage_commercial_id.clone())
            .build()?,
        Entity::builder()
            .id(car_x1_id)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", garage_industrial_id.clone())
            .build()?,
        Entity::builder()
            .id(car_x2_id)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", garage_industrial_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("category").eq("commercial"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .limit(2)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.total(), some(eq(3)));
    expect_that!(page.items(), len(eq(2)));
    expect_that!(
        page.items().first().map(|item| item.id.to_string()),
        some(contains_regex(format!("^urn:ngsi-ld:Car:{namespace}-m[1-3]$")))
    );
    expect_that!(
        page.items().get(1).map(|item| item.id.to_string()),
        some(contains_regex(format!("^urn:ngsi-ld:Car:{namespace}-m[1-3]$")))
    );
    Ok(())
}
