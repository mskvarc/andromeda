//! A registration under a scoped body context is bad request data.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    core_context::core_entry,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, ErrorType, InlineContext, NgsiLdMediaType},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration whose body @context defines gear with a scoped @context answers 400
/// `BadRequestData` and stores nothing, and with a plain one 201
/// Clause: 5.5.7, 5.9.2.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9045_a_registration_under_a_scoped_body_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let body_context = broker.with_context(Context::Multiple(vec![core_entry()?, ContextEntry::Inline(format!(r#"{{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}"#).parse::<InlineContext>()?)]))?.with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_scoped_id = namespace.registration_id("scoped")?;
    let device_type = namespace.entity_type("Device")?;
    let body_context_2 = broker
        .with_context(Context::Multiple(vec![
            core_entry()?,
            ContextEntry::Inline(format!(r#"{{"gear": "https://example.org/{namespace}/gear"}}"#).parse::<InlineContext>()?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_plain_id = namespace.registration_id("plain")?;
    let registration_scoped = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_scoped_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    let answer = body_context.with_responses().registrations().register(&registration_scoped).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_scoped_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    let registration_plain = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_plain_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .build()?;
    body_context_2.registrations().register(&registration_plain).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_plain_cleanup = Cleanup::of(&body_context_2).registration(&registration_plain_id);
    broker.registrations().delete(&registration_plain_id).send().await?;
    Ok(())
}
