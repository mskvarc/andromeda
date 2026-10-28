//! ETSI `020_07`: the temporal evolution of an entity that does not exist is refused with 404 `ResourceNotFound`
//! (clause 5.7.3).

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, model::ErrorType};

/// Pins: Check that one cannot retrieve the temporal evolution of a non-existing entity
/// Clause: 5.7.3
/// Vocabulary: none (the ETSI case references no @context)
#[gtest]
#[tokio::test]
async fn e020_07_01_retrieve_the_temporal_evolution_of_a_non_existing_entity() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = namespace.urn("Vehicle", "unknowEntity")?;
    let received = broker.with_accept(Accept::any()).with_responses().temporal().retrieve(&id).send().await;
    verify_that!(&received, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title())
}
