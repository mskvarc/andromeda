//! A registration forwarding context declaring a scoped context is bad request data.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, Input, NgsiLdMediaType},
    registration::{CSourceRegistration, ContextSourceInfo, EntityInfo, RegistrationFragment, RegistrationId, RegistrationInfo, RegistrationTypeMember},
};

/// Pins: Create and Update Context Source Registration whose `contextSourceInfo` `jsonldContext` names a context
/// defining gear with a scoped @context answer 400 `BadRequestData` and store nothing, while one naming a plain
/// context is created and keeps it
/// Clause: 4.3.6.6, 4.4, 5.5.7, 5.9.2.4, 5.9.3.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9055_a_registration_forwarding_context_declaring_a_scoped_context_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve(
            "contexts/scoped.jsonld",
            &format!(
                r#"{{"@context": {{"gear": {{"@id": "https://example.org/{namespace}/gear", "@context": {{"observedAt": "https://example.org/{namespace}/rewritten"}}}}}}}}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    contexts
        .serve(
            "contexts/plain.jsonld",
            &format!(r#"{{"@context": {{"gear": "https://example.org/{namespace}/gear"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let scoped = contexts.url("contexts/scoped.jsonld")?;
    let plain = contexts.url("contexts/plain.jsonld")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let scoped_id = namespace.registration_id("scoped")?;
    let plain_id = namespace.registration_id("plain")?;
    let registration = |id: RegistrationId, jsonld_context: &str| -> Result<CSourceRegistration<Input>> {
        Ok(CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(id)
            .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build()?])
            .context_source_info(("jsonldContext", jsonld_context))
            .build()?)
    };
    let scoped_registration = registration(scoped_id.clone(), scoped.as_str())?;
    let refused = broker.with_responses().registrations().register(&scoped_registration).send().await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let absent = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&scoped_id)
        .send()
        .await;
    expect_that!(&absent, answered(StatusCode::NOT_FOUND));
    let plain_registration = registration(plain_id.clone(), plain.as_str())?;
    broker.registrations().register(&plain_registration).send().await?;
    let fragment = RegistrationFragment::builder()
        .context_source_info([("jsonldContext", scoped.as_str())])
        .type_member(RegistrationTypeMember::Omitted)
        .build()?;
    let answer = broker.with_responses().registrations().update(&plain_id, &fragment).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let kept = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&plain_id)
        .send()
        .await?;
    expect_that!(kept.context_source_info().map(<[_]>::len), some(eq(1)));
    expect_that!(
        kept.context_source_info(),
        some(contains(eq(&ContextSourceInfo::parse("jsonldContext", plain.as_str())?)))
    );
    let deleted = broker.with_responses().registrations().delete(&plain_id).send().await;
    expect_that!(&deleted, answered(StatusCode::NO_CONTENT));
    Ok(())
}
