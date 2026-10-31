//! A registration under an unretrievable context is ld context not available.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: an application/ld+json Register Context Source whose @context cannot be retrieved answers 504
/// `LdContextNotAvailable` and creates no registration
/// Clause: 5.5.6, 6.3.2, 5.9.2
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0013_a_registration_under_an_unretrievable_context_is_ld_context_not_available() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let unreachable = config.unreachable_origin().origin().ascii_serialization();
    let body_context = broker
        .with_context(Context::Multiple(vec![ContextEntry::Reference(IriRefBuf::new(format!(
            "{unreachable}/contexts/{namespace}/missing.jsonld"
        ))?)]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([building_type.as_str()]).build()?])
        .build()?;
    let answer = body_context.with_responses().registrations().register(&registration).send().await;
    expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
