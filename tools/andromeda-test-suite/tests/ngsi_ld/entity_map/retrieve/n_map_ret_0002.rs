//! An unknown map is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    registration::EntityMapId,
};

/// Pins: Retrieve `EntityMap` of an identifier no `EntityMap` has answers 404 `ResourceNotFound`
/// Clause: 5.14.1.4, 6.32.3.1
#[gtest]
#[tokio::test]
async fn n_map_ret_0002_an_unknown_map_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let unknown_id = format!("urn:ngsi-ld:EntityMap:{namespace}-unknown").parse::<EntityMapId>()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .retrieve(&unknown_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
