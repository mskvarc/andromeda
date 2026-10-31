//! An unknown entity is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{AttributeName, ErrorType};

/// Pins: Delete Attribute addressed to an Entity id no Temporal Evolution has is refused with 404
/// `ResourceNotFound`, with a title
/// Clause: 5.6.13.4, 6.21.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_adl_0007_an_unknown_entity_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_unknown_id = namespace.urn("Vehicle", "unknown")?;
    let answer = broker
        .with_responses()
        .temporal()
        .delete_attribute(&vehicle_unknown_id, &"fuelLevel".parse::<AttributeName>()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    Ok(())
}
