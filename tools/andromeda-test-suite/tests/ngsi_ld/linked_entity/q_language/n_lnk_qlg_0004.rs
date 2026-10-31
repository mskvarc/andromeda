//! A hop without a comparison tests existence.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::{Join, q},
};

/// Pins: q R{attr} with no comparison is an existence test on the Linked Entity: the root is selected when the
/// entity its Relationship names holds the attribute, whatever its value
/// Clause: 4.9, 4.5.23, 5.7.2.4
#[gtest]
#[tokio::test]
async fn n_lnk_qlg_0004_a_hop_without_a_comparison_tests_existence() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_categorised_id = namespace.urn("Building", "categorised")?;
    let building_type = namespace.entity_type("Building")?;
    let building_plain_id = namespace.urn("Building", "plain")?;
    let car_categorised_id = namespace.urn("Car", "categorised")?;
    let car_type = namespace.entity_type("Car")?;
    let car_plain_id = namespace.urn("Car", "plain")?;
    let entities = [
        Entity::builder()
            .id(building_categorised_id.clone())
            .entity_type(building_type.clone())
            .property("category", "industrial")
            .build()?,
        Entity::builder()
            .id(building_plain_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Depot")
            .build()?,
        Entity::builder()
            .id(car_categorised_id.clone())
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_categorised_id.clone())
            .build()?,
        Entity::builder()
            .id(car_plain_id)
            .entity_type(car_type.clone())
            .relationship("isParkedIn", building_plain_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&car_type)
        .q(q::attr("isParkedIn").linked("category").exists())
        .join(Join::Inline)
        .join_level(1.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&car_categorised_id)));
    Ok(())
}
