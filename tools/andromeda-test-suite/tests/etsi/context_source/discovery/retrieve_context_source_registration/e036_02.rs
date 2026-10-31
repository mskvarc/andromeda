//! A retrieval of a context source registration the broker does not hold is refused with 404 (clause 5.10.1).

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::Accept, model::ErrorType, registration::RegistrationId};

/// Pins: Check that one cannot retrieve a Context Source Registration, if the NGSI-LD endpoint does not know about the
/// target context source registration, because there is no existing context source registration whose id (URI) is
/// equivalent
/// Clause: 5.10.1
/// Vocabulary: none (the test references no @context)
#[gtest]
#[tokio::test]
async fn e036_02_01_retrieve_unknown_context_source_registration() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id: RegistrationId = namespace.urn_text("ContextSourceRegistration", "unknowRegistration").parse()?;
    let retrieved = broker.with_accept(Accept::any()).with_responses().registrations().retrieve(&id).send().await;
    expect_that!(&retrieved, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    Ok(())
}
