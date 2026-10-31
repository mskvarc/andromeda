//! A hop does not cross a deleted relationship.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, type_matcher::has_type};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: a q hop follows only the Relationships an entity holds now: two Cars reach the commercial Building, and
/// once one Car's Relationship is deleted the hop selects the other alone
/// Clause: 4.9, 4.5.23, 5.6.5, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0007_a_hop_does_not_cross_a_deleted_relationship() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_commercial_id = namespace.urn("Building", "commercial")?;
    let building_type = namespace.entity_type("Building")?;
    let car_live_id = namespace.urn("Car", "live")?;
    let car_type = namespace.entity_type("Car")?;
    let car_deleted_id = namespace.urn("Car", "deleted")?;
    let entities = [
        Entity::builder()
            .id(building_commercial_id.clone())
            .entity_type(building_type)
            .property("category", "commercial")
            .build()?,
        Entity::builder()
            .id(car_live_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_commercial_id.clone())
            .build()?,
        Entity::builder()
            .id(car_deleted_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_commercial_id.clone())
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
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(all!(has_id(&car_live_id), has_type(&car_type))));
    expect_that!(page.items(), contains(all!(has_id(&car_deleted_id), has_type(&car_type))));
    broker
        .entities()
        .delete_attribute(&car_deleted_id, &"isParkedIn".parse::<AttributeName>()?)
        .send()
        .await?
        .done()?;
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("category").eq("commercial"))
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&car_live_id)));
    Ok(())
}
