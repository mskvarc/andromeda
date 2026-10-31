//! An unknown entity is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{Attribute, AttributeName, ErrorType, Property};

/// Pins: Replace Attribute addressed to an Entity the broker does not hold is refused with 404 `ResourceNotFound`
/// Clause: 5.6.19.4, 6.7.3.3
#[gtest]
#[tokio::test]
async fn n_att_rpl_0007_an_unknown_entity_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_unknown_id = namespace.urn("Vehicle", "unknown")?;
    let attribute = Attribute::from(Property::builder(100).build()?);
    let answer = broker
        .with_responses()
        .entities()
        .replace_attribute(&vehicle_unknown_id, &"speed".parse::<AttributeName>()?, &attribute)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
