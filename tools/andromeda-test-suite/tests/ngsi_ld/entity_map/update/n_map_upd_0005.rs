//! An unknown map is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{model::ErrorType, registration::EntityMapId};

/// Pins: Update `EntityMap` of an identifier no `EntityMap` has answers 404 `ResourceNotFound`
/// Clause: 5.14.2.4, 6.32.3.2
#[gtest]
#[tokio::test]
async fn n_map_upd_0005_an_unknown_map_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let unknown_id = format!("urn:ngsi-ld:EntityMap:{namespace}-unknown").parse::<EntityMapId>()?;
    let answer = broker
        .with_responses()
        .entity_maps()
        .update(&unknown_id, &"2099-01-01T00:00:00Z".parse()?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
