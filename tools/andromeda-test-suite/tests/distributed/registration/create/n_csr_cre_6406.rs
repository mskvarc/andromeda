//! A context source info ngsild conformance must be major dot minor.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, RawJson},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Context Source Registration with `contextSourceInfo` `ngsildConformance` 1.5 answers 201, while 1
/// and 1.5.0 answer 400 `BadRequestData`
/// Clause: 4.3.6.6, 5.9.2.4
#[gtest]
#[tokio::test]
async fn n_csr_cre_6406_a_context_source_info_ngsild_conformance_must_be_major_dot_minor() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration_3_id = namespace.registration_id(3)?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info: RawJson = r#"[{"key": "ngsildConformance", "value": "1.5"}]"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .build()?;
    // The model refuses it (the document breaks the rules of its data type at contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info_2: RawJson = r#"[{"key": "ngsildConformance", "value": "1"}]"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("contextSourceInfo", &context_source_info_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_3 = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_3_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .build()?;
    // The model refuses it (the document breaks the rules of its data type at contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info_3: RawJson = r#"[{"key": "ngsildConformance", "value": "1.5.0"}]"#.parse()?;
    let document_3 = UncheckedDocument::try_from(registration_3.render())?.set_member("contextSourceInfo", &context_source_info_3);
    let request_3 = broker
        .unchecked(&RegisterContextSource { registration: &registration_3 })?
        .document(&document_3);
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_2_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    Ok(())
}
