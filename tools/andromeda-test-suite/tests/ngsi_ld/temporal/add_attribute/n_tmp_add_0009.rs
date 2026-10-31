//! An unknown entity is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{ErrorType, Property, evolution::TemporalEntity};

/// Pins: Add Attributes addressed to an Entity id no Temporal Evolution has is refused with 404 `ResourceNotFound`,
/// with a title
/// Clause: 5.6.12.4, 6.20.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_add_0009_an_unknown_entity_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_unknown_id = namespace.urn("Vehicle", "unknown")?;
    let instances = TemporalEntity::builder()
        .id(vehicle_unknown_id.clone())
        .entity_type("Placeholder")
        .property("added_attribute", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("added_attribute", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("added_attribute", Property::builder(100).observed_at("2020-09-01T12:07:00Z".parse()?))
        .build()?
        .attributes;
    let answer = broker.with_responses().temporal().add_attributes(&vehicle_unknown_id, &instances).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    Ok(())
}
