//! Delete on the registration collection is method not allowed.

use andromeda_test_suite::{broker, problem::answered, wire::request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: DELETE on the `csourceRegistrations` collection of broker A, which names no registration, answers 405
/// Clause: 5.9.4, 6.3.2
#[gtest]
#[tokio::test]
async fn n_csr_del_7600_delete_on_the_registration_collection_is_method_not_allowed() -> Result<()> {
    let broker = broker::front()?;
    // Delete Context Source Registration names a registration; the collection has no such operation, so the request is sent as written.
    let request = request_to(&broker, Method::DELETE, "csourceRegistrations")?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::METHOD_NOT_ALLOWED));
    Ok(())
}
