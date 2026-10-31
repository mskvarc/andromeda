//! A q through a linked entity without a join is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::q,
};

/// Pins: Create `EntityMap` for Query Entities with a q over a Linked Entity attribute and no join answers 400
/// `BadRequestData` as too deep a query
/// Clause: 5.14.4.4, 4.9, 4.5.23
#[gtest]
#[tokio::test]
async fn n_map_qen_7002_a_q_through_a_linked_entity_without_a_join_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entity_maps()
        .create()
        .entity_type(&namespace.entity_type("Vehicle")?)
        .q(q::attr("linkedTo").linked("speed").gt(5))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
