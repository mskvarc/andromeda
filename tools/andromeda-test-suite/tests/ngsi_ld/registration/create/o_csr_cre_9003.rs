//! A forwarding context is dereferenced when the registration is created.

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
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, ContextSourceInfo, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration resolves and processes the `jsonldContext` its `contextSourceInfo`
/// names: one that cannot be retrieved answers 504 `LdContextNotAvailable`, one that is not an absolute URL or
/// retrieves as no valid @context answers 400 `BadRequestData`, none of the three creates the registration, and one
/// that loads and processes is created and reads back
/// Clause: 4.3.6.6, 5.5.6, 5.9.2.4, 6.3.19
/// Recorded: clause 4.3.6.6 has every request distributed to the registration compacted against this context and
/// does not say when it is first fetched; Andromeda dereferences it at the write, as clauses 5.8.1.4 and 5.8.2.4 do
/// for a subscription's `jsonldContext`, so an unusable one fails the registration instead of every forwarded
/// request, and an absolute URL is required because a registration has no base a relative reference could resolve
/// against
#[gtest]
#[tokio::test]
async fn o_csr_cre_9003_a_forwarding_context_is_dereferenced_when_the_registration_is_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let contexts = RemoteContexts::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let absent = namespace.registration_id("absent")?;
    let relative = namespace.registration_id("relative")?;
    let invalid = namespace.registration_id("invalid")?;
    let valid = namespace.registration_id("valid")?;
    let valid_url = contexts
        .serve(
            "contexts/valid.jsonld",
            &format!(r#"{{"@context": {{"speed": "https://example.org/{namespace}/speed"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let invalid_url = contexts
        .serve("contexts/invalid.jsonld", r#"{"@context": {"speed": 42}}"#, Some("application/ld+json"))
        .await?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let absent_url = format!("{unreachable}/contexts/{namespace}/absent.jsonld");
    let relative_url = format!("/contexts/{namespace}/valid.jsonld");
    let attempts = [
        (&absent, absent_url.as_str(), Some(StatusCode::GATEWAY_TIMEOUT)),
        (&relative, relative_url.as_str(), Some(StatusCode::BAD_REQUEST)),
        (&invalid, invalid_url.as_str(), Some(StatusCode::BAD_REQUEST)),
        (&valid, valid_url.as_str(), None),
    ];
    for (registration_id, jsonld_context, refusal) in attempts {
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(registration_id.clone())
            .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build()?])
            .build()?;
        // The forwarding context is a `contextSourceInfo` pair whose value the model need not accept, so it is written as text.
        let source_info: RawJson = format!(r#"[{{"key": "jsonldContext", "value": "{jsonld_context}"}}]"#).parse()?;
        let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &source_info);
        let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
        let answer = broker.send_unchecked(request).await;
        match refusal {
            Some(StatusCode::GATEWAY_TIMEOUT) => expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable)),
            Some(status) => expect_that!(&answer, problem(status, ErrorType::BadRequestData)),
            None => expect_that!(&answer, answered(StatusCode::CREATED)),
        }
        if refusal.is_some() {
            let missing = broker
                .with_responses()
                .with_accept(Accept::new(NgsiLdMediaType::Json))
                .registrations()
                .retrieve(registration_id)
                .send()
                .await;
            expect_that!(&missing, answered(StatusCode::NOT_FOUND));
        }
    }
    let stored = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&valid)
        .send()
        .await?;
    expect_that!(
        stored.context_source_info(),
        some(elements_are![eq(&ContextSourceInfo::parse("jsonldContext", valid_url.as_str())?)])
    );
    broker.registrations().delete(&valid).send().await?;
    Ok(())
}
