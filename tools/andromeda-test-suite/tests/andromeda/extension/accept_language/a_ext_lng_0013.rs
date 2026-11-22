//! Retrieve context source registration ignores the accepted language.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    context_source_property::language_property_value,
    namespace::Namespace,
    problem::answered,
    wire::vary,
};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::headers::{Accept, AcceptLanguage},
    model::NgsiLdMediaType,
    query::language_filter::LanguageFilter,
    registration::{CSourceRegistration, ContextSourcePropertyName, EntityInfo, RegistrationInfo},
};

/// Pins: Retrieve Context Source Registration with Accept-Language fr answers the label `LanguageProperty` of the
/// registration with its whole `languageMap`, and the response does not vary on accept-language
/// Clause: 4.15, 5.10.1, 6.9.3.1
/// Recorded: Andromeda extension, not v1.9.1: the `accept_language` extension reads Accept-Language as the clause
/// 4.15 language filter only where an operation takes lang, and Retrieve Context Source Registration takes none
#[gtest]
#[tokio::test]
async fn a_ext_lng_0013_retrieve_context_source_registration_ignores_the_accepted_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let label = language_property_value(&[("fr", "Source principale"), ("nl", "Hoofdbron")]);
    let registration = CSourceRegistration::builder(source.endpoint(&format!("sources/{namespace}"))?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id(namespace.urn("Building", "remote")?)])
            .build()?])
        .property("label", label.clone())
        .build()?;
    let created = broker.with_responses().registrations().register(&registration).send().await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let answer = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_header(AcceptLanguage::try_from("fr".parse::<LanguageFilter>()?)?)?
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::OK));
    let headed = answer?;
    expect_that!(vary(headed.head.headers()), not(contains(eq(&HeaderName::from_static("accept-language")))));
    expect_that!(headed.outcome.id(), eq(&registration_id));
    expect_that!(
        headed.outcome.properties().get(&"label".parse::<ContextSourcePropertyName>()?),
        some(eq(&label))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
