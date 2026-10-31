//! A management member outside its definition is bad request data.

use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, operation::RegisterContextSource},
    model::{ErrorType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration with a management `cacheDuration` that is no ISO 8601 duration, or a
/// cooldown or timeout that is zero or negative, answers 400 `BadRequestData`
/// Clause: 5.9.2.4, 5.2.34
#[gtest]
#[tokio::test]
async fn n_csr_cre_6010_a_management_member_outside_its_definition_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let registration_4_id = namespace.registration_id(4)?;
    let registration_5_id = namespace.registration_id(5)?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid duration at management.cacheDuration), so `management` is written as text.
    let management: RawJson = r#"{"cacheDuration": "10s"}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("management", &management);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_2 = CSourceRegistration::builder(source.endpoint("2")?)
        .id(registration_2_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid number at management.cooldown), so `management` is written as text.
    let management_2: RawJson = r#"{"cooldown": 0}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("management", &management_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_3 = CSourceRegistration::builder(source.endpoint("3")?)
        .id(registration_3_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid number at management.cooldown), so `management` is written as text.
    let management_3: RawJson = r#"{"cooldown": -5}"#.parse()?;
    let document_3 = UncheckedDocument::try_from(registration_3.render())?.set_member("management", &management_3);
    let request_3 = broker
        .unchecked(&RegisterContextSource { registration: &registration_3 })?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_4 = CSourceRegistration::builder(source.endpoint("4")?)
        .id(registration_4_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid number at management.timeout), so `management` is written as text.
    let management_4: RawJson = r#"{"timeout": 0}"#.parse()?;
    let document_4 = UncheckedDocument::try_from(registration_4.render())?.set_member("management", &management_4);
    let request_4 = broker
        .unchecked(&RegisterContextSource { registration: &registration_4 })?
        .document(&document_4);
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_5 = CSourceRegistration::builder(source.endpoint("5")?)
        .id(registration_5_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .build()?;
    // The model refuses it (invalid number at management.timeout), so `management` is written as text.
    let management_5: RawJson = r#"{"timeout": -1}"#.parse()?;
    let document_5 = UncheckedDocument::try_from(registration_5.render())?.set_member("management", &management_5);
    let request_5 = broker
        .unchecked(&RegisterContextSource { registration: &registration_5 })?
        .document(&document_5);
    let answer_5 = broker.send_unchecked(request_5).await;
    expect_that!(&answer_5, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
