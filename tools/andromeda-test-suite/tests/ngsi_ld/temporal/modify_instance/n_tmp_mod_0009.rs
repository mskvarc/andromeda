//! An unknown entity is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{Attribute, AttributeName, ErrorType, InstanceId, Property};

/// Pins: Modify Attribute instance addressed to an Entity id no Temporal Evolution has is refused with 404
/// `ResourceNotFound`, with a title
/// Clause: 5.6.14.4, 6.22.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_mod_0009_an_unknown_entity_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_unknown_id = namespace.urn("Vehicle", "unknown")?;
    let instance_id = format!("urn:ngsi-ld:Instance:{namespace}-unknown").parse::<InstanceId>()?;
    let instance = Attribute::from(Property::builder(129).observed_at("2020-09-01T12:03:00Z".parse()?).build()?);
    let answer = broker
        .with_responses()
        .temporal()
        .modify_instance(&vehicle_unknown_id, &"speed".parse::<AttributeName>()?, &instance_id, &instance)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    Ok(())
}
