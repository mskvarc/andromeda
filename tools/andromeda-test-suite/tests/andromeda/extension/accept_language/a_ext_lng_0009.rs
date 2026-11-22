//! Query context source registrations ignores the accepted language.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    context_source_property::language_property_value,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
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

/// Pins: Query Context Source Registrations with Accept-Language fr and no lang answers the label `LanguageProperty`
/// of a registration with its whole `languageMap`, and the response does not vary on accept-language
/// Clause: 4.15, 5.10.2.5, 6.8.3.2
/// Recorded: Andromeda extension, not v1.9.1: the `accept_language` extension reads Accept-Language as the clause
/// 4.15 language filter where no lang names one, and lang leaves the registrations of Query Context Source
/// Registrations as they were registered, so the header has no effect there to stand in for
#[gtest]
#[tokio::test]
async fn a_ext_lng_0009_query_context_source_registrations_ignores_the_accepted_language() -> Result<()> {
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
        .query()
        .entity_type(&building_type)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::OK));
    let headed = answer?;
    expect_that!(vary(headed.head.headers()), not(contains(eq(&HeaderName::from_static("accept-language")))));
    let registrations = headed.outcome.items();
    expect_that!(registrations, len(eq(1)));
    expect_that!(registrations, nth(0, has_id(&registration_id)));
    let label_name = "label".parse::<ContextSourcePropertyName>()?;
    expect_that!(
        registrations.first().and_then(|registration| registration.properties().get(&label_name)),
        some(eq(&label))
    );
    Ok(())
}
