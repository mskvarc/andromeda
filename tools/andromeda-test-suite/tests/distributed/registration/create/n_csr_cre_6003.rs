//! An expires at in the past is bad request data.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::ErrorType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with an `expiresAt` in the past answers 400 `BadRequestData` and
/// creates nothing
/// Clause: 5.9.2.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_cre_6003_an_expires_at_in_the_past_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .expires_at("2000-01-01T00:00:00Z".parse()?)
        .build()?;
    let answer = broker.with_responses().registrations().register(&registration).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
