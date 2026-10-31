//! An unknown registration is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::ErrorType;

/// Pins: Delete Context Source Registration of an id no registration holds answers 404 `ResourceNotFound`
/// Clause: 5.9.4.4
#[gtest]
#[tokio::test]
async fn n_csr_del_6001_an_unknown_registration_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let registration_unknown_id = namespace.registration_id("unknown")?;
    let answer = broker.with_responses().registrations().delete(&registration_unknown_id).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
