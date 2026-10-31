//! A management interval with a numeric offset is bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Register Context Source whose `managementInterval` `endAt` has a numeric offset in place of Z (+01:00, or
/// +00:00 naming UTC) answers 400 `BadRequestData`, and no registration exists
/// Clause: 4.6.3, 5.2.9, 5.2.11, 5.9.2.4, 5.5.4
#[gtest]
#[tokio::test]
async fn n_csr_cre_7611_a_management_interval_with_a_numeric_offset_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid timestamp at managementInterval.endAt), so `managementInterval` is written as text.
    let management_interval: RawJson = r#"{"startAt": "2026-01-01T00:00:00Z", "endAt": "2026-03-04T05:06:07+01:00"}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("managementInterval", &management_interval);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid timestamp at managementInterval.endAt), so `managementInterval` is written as text.
    let management_interval_2: RawJson = r#"{"startAt": "2026-01-01T00:00:00Z", "endAt": "2026-03-04T05:06:07+00:00"}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("managementInterval", &management_interval_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
