//! A batch merge is read back.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment},
};

/// Pins: Batch Entity Merge on broker A of an Entity it holds answers 204 and the merged speed reads back
/// Clause: 5.6.20.4, 6.31.3.1
#[gtest]
#[tokio::test]
async fn n_bat_mrg_7600_a_batch_merge_is_read_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragments = [EntityFragment::for_entity(vehicle_id.clone(), vehicle_type.clone())
        .property("speed", 99)
        .build()?];
    broker.entities().batch_merge(&fragments).send().await?.done()?;
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(99)));
    Ok(())
}
