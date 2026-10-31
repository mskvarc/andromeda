//! A flat join over a cycle answers each entity once.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    query::Join,
};

/// Pins: Query Entities with join=flat over a Relationship cycle (x -> y -> x) under `joinLevel=5` terminates and
/// answers each entity of the cycle exactly once, the matching entity first
/// Clause: 4.5.23.1, 4.5.23.3, 5.7.2.5
#[gtest]
#[tokio::test]
async fn n_lnk_qry_0010_a_flat_join_over_a_cycle_answers_each_entity_once() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_x_id = namespace.urn("Building", "x")?;
    let building_type = namespace.entity_type("Building")?;
    let building_y_id = namespace.urn("Building", "y")?;
    let entities = [
        Entity::builder()
            .id(building_x_id.clone())
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_y_id.clone())
            .build()?,
        Entity::builder()
            .id(building_y_id.clone())
            .entity_type(building_type.clone())
            .relationship("linkedTo", building_x_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_x_id.clone()])
        .entity_type(&building_type)
        .join(Join::Flat)
        .join_level(5.try_into()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), nth(0, has_id(&building_x_id)));
    expect_that!(page.items(), nth(1, has_id(&building_y_id)));
    Ok(())
}
