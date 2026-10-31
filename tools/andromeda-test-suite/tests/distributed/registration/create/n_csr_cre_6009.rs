//! A time interval without start at is bad request data.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::{ErrorType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with a `managementInterval` that has an `endAt` and no `startAt`
/// answers 400 `BadRequestData`
/// Clause: 5.9.2.4, 5.2.9, 5.2.11
#[gtest]
#[tokio::test]
async fn n_csr_cre_6009_a_time_interval_without_start_at_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (missing member `startAt` at managementInterval), so `managementInterval` is written as text.
    let management_interval: RawJson = r#"{"endAt": "2030-01-01T00:00:00Z"}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("managementInterval", &management_interval);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_2 = CSourceRegistration::builder(source.endpoint("2")?)
        .id(registration_2_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (missing member `startAt` at observationInterval), so `observationInterval` is written as text.
    let observation_interval: RawJson = r#"{"endAt": "2030-01-01T00:00:00Z"}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("observationInterval", &observation_interval);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
