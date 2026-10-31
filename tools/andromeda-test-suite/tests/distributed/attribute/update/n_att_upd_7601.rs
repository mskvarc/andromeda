//! A change is read back and missing targets are refused.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, ErrorType},
};

/// Pins: Update Entity Attributes on broker A of an Entity it holds answers 204 and the change reads back; of an
/// unknown Entity it answers 404 `ResourceNotFound`
/// Clause: 5.6.2.4, 6.6.3.2
#[gtest]
#[tokio::test]
async fn n_att_upd_7601_a_change_is_read_back_and_missing_targets_are_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 5)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let fragment = EntityFragment::builder().property("speed", 50).build()?;
    broker.entities().update_attributes(&vehicle_id, &fragment).send().await?.done()?;
    let retrieved = broker.with_accept(Accept::any()).entities().retrieve(&vehicle_id).send().await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().value(50)));
    let fragment_2 = EntityFragment::builder().property("speed", 50).build()?;
    let answer = broker.with_responses().entities().update_attributes(&vehicle_2_id, &fragment_2).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
