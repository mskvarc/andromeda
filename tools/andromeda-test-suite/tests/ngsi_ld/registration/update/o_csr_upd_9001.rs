//! A replaced forwarding context is dereferenced and an unusable one changes nothing.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    problem::{answered, problem},
    remote_context::RemoteContexts,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{ErrorType, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, ContextSourceInfo, EntityInfo, RegistrationId, RegistrationInfo},
};

/// Pins: Update Context Source Registration whose fragment replaces `contextSourceInfo` resolves and processes the
/// `jsonldContext` it names: one that cannot be retrieved answers 504 `LdContextNotAvailable`, one that retrieves
/// as no valid @context answers 400 `BadRequestData`, neither changes the stored `contextSourceInfo`, and one that
/// loads and processes replaces it
/// Clause: 4.3.6.6, 5.5.6, 5.9.3.4, 6.3.19
/// Recorded: Andromeda dereferences the `contextSourceInfo` `jsonldContext` at the write, as clauses 5.8.1.4 and
/// 5.8.2.4 do for a subscription's `jsonldContext`; an update whose fragment leaves `contextSourceInfo` out does
/// not dereference the stored one again
#[gtest]
#[tokio::test]
async fn o_csr_upd_9001_a_replaced_forwarding_context_is_dereferenced_and_an_unusable_one_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let contexts = RemoteContexts::start().await?;
    let registration_id = format!("urn:ngsi-ld:ContextSourceRegistration:{namespace}").parse::<RegistrationId>()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let first = contexts
        .serve(
            "contexts/first.jsonld",
            &format!(r#"{{"@context": {{"speed": "https://example.org/{namespace}/speed"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let second = contexts
        .serve(
            "contexts/second.jsonld",
            &format!(r#"{{"@context": {{"velocity": "https://example.org/{namespace}/speed"}}}}"#),
            Some("application/ld+json"),
        )
        .await?;
    let invalid = contexts
        .serve("contexts/invalid.jsonld", r#"{"@context": {"speed": 42}}"#, Some("application/ld+json"))
        .await?;
    let absent = format!(
        "{}/contexts/{namespace}/absent.jsonld",
        config.unreachable_origin().origin().ascii_serialization()
    );
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(vehicle_type.clone())]).build()?])
        .build()?;
    // The forwarding context is a `contextSourceInfo` pair whose value the model need not accept, so it is written as text.
    let first_info: RawJson = format!(r#"[{{"key": "jsonldContext", "value": "{first}"}}]"#).parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &first_info);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let path = format!("csourceRegistrations/{registration_id}");
    let patch = |value: String| format!(r#"{{"contextSourceInfo": [{{"key": "jsonldContext", "value": "{value}"}}]}}"#);
    let request_2 = json_request_to(&broker, Method::PATCH, &path, &patch(absent))?;
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let request_3 = json_request_to(&broker, Method::PATCH, &path, &patch(invalid.to_string()))?;
    let answer_3 = broker.send_unchecked(request_3).await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let kept = json.registrations().retrieve(&registration_id).send().await?;
    expect_that!(
        kept.context_source_info(),
        some(elements_are![eq(&ContextSourceInfo::parse("jsonldContext", first.as_str())?)])
    );
    let request_4 = json_request_to(&broker, Method::PATCH, &path, &patch(second.to_string()))?;
    let answer_4 = broker.send_unchecked(request_4).await;
    expect_that!(&answer_4, answered(StatusCode::NO_CONTENT));
    let replaced = json.registrations().retrieve(&registration_id).send().await?;
    expect_that!(
        replaced.context_source_info(),
        some(elements_are![eq(&ContextSourceInfo::parse("jsonldContext", second.as_str())?)])
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
