//! A context source info jsonld context must be a url.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
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

/// Pins: Create Context Source Registration with `contextSourceInfo` `jsonldContext` that is not a URL reference
/// answers 400 `BadRequestData`, while the core context URL answers 201
/// Clause: 4.3.6.6, 5.9.2.4
#[gtest]
#[tokio::test]
async fn n_csr_cre_6405_a_context_source_info_jsonld_context_must_be_a_url() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let config = SuiteConfig::get()?;
    let core = config.core_context();
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .build()?;
    // The model refuses it (the document breaks the rules of its data type at contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info: RawJson = r####"[{"key": "jsonldContext", "value": "### not a url ###"}]"####.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_2 = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info_2: RawJson = format!(r#"[{{"key": "jsonldContext", "value": "{core}"}}]"#).parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("contextSourceInfo", &context_source_info_2);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let answer_3 = broker
        .with_responses()
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(&answer_3, answered(StatusCode::NOT_FOUND));
    Ok(())
}
