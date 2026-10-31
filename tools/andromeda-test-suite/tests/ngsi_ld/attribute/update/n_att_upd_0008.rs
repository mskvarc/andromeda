//! An unknown entity is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{EntityFragment, ErrorType};

/// Pins: Update Attributes addressed to an Entity the broker does not hold is refused with 404 `ResourceNotFound`
/// Clause: 5.6.2.4, 6.6.3.2
#[gtest]
#[tokio::test]
async fn n_att_upd_0008_an_unknown_entity_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_unknown_id = namespace.urn("Vehicle", "unknown")?;
    let fragment = EntityFragment::builder().property("speed", 100).build()?;
    let answer = broker
        .with_responses()
        .entities()
        .update_attributes(&vehicle_unknown_id, &fragment)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
