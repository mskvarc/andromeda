//! An unknown map is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{model::ErrorType, registration::EntityMapId};

/// Pins: Delete `EntityMap` of an identifier no `EntityMap` has answers 404 `ResourceNotFound`
/// Clause: 5.14.3.4, 6.32.3.3
#[gtest]
#[tokio::test]
async fn n_map_del_0002_an_unknown_map_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let unknown_id = format!("urn:ngsi-ld:EntityMap:{namespace}-unknown").parse::<EntityMapId>()?;
    let answer = broker.with_responses().entity_maps().delete(&unknown_id).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
