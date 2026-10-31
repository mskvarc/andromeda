//! Updating every named attribute answers 204 without a body.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::model::{Entity, EntityFragment};

/// Pins: Update Attributes of the only attribute the Entity carries answers 204 with no `UpdateResult` body
/// Clause: 5.6.2.5
#[gtest]
#[tokio::test]
async fn n_att_upd_9004_updating_every_named_attribute_answers_204_without_a_body() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_update_complete_id = namespace.urn("Vehicle", "update-complete")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_update_complete = Entity::builder()
        .id(vehicle_update_complete_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 10)
        .build()?;
    broker.entities().create(&vehicle_update_complete).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 55).build()?;
    let received = broker
        .with_responses()
        .entities()
        .update_attributes(&vehicle_update_complete_id, &fragment)
        .send()
        .await?;
    received.outcome.done()?;
    expect_that!(received.body.len(), eq(0));
    Ok(())
}
