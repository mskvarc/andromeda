//! A fragment under an unreachable context is ld context not available and changes nothing.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    namespace::Namespace,
    problem::problem,
    registration_vocabulary::{registration_vocabulary, vehicle_and_parking_registration},
    unreachable_context::unreachable_context,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ErrorType, NgsiLdMediaType},
    registration::{RegistrationFragment, RegistrationTypeMember},
};
use url::Url;

/// Pins: Update Context Source Registration with an application/json fragment under a Link naming an @context
/// nothing serves answers 504 `LdContextNotAvailable`, and the registration then reads back with its endpoint
/// unchanged
/// Clause: 5.9.3, 5.5.6, 6.3.5
#[gtest]
#[tokio::test]
async fn n_csr_upd_0005_a_fragment_under_an_unreachable_context_is_ld_context_not_available_and_changes_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let ctx = HostedContext::add(&broker, &registration_vocabulary(&namespace)?).await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![ctx.entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_id = namespace.registration_id(1)?;
    let registration = vehicle_and_parking_registration(&namespace, &registration_id, None)?;
    body_context.registrations().register(&registration).send().await?;
    let unreachable_linked = broker.with_context(unreachable_context("unreachable.jsonld")?)?;
    let fragment = RegistrationFragment::builder()
        .endpoint("http://unreachable.example.org:1026")
        .type_member(RegistrationTypeMember::Omitted)
        .build()?;
    let answer = unreachable_linked
        .with_responses()
        .registrations()
        .update(&registration_id, &fragment)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.endpoint(), eq(&"http://my.csource.org:1026/".parse::<Url>()?));
    Ok(())
}
