//! Query context source registrations answers the accepted language.

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
    model::{NgsiLdMediaType, Value},
    query::language_filter::LanguageFilter,
    registration::{CSourceRegistration, ContextSourcePropertyName, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations with Accept-Language fr and no lang answers the label `LanguageProperty`
/// of a registration as a Property holding the French string with lang fr, and the response varies on
/// accept-language
/// Clause: 4.15, 5.10.2.4
/// Recorded: Andromeda extension, not v1.9.1: the `accept_language` extension reads Accept-Language as the clause
/// 4.15 language filter where no lang names one; NGSI-LD carries the filter in lang alone
#[gtest]
#[tokio::test]
async fn a_ext_lng_0009_query_context_source_registrations_answers_the_accepted_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(source.endpoint(&format!("sources/{namespace}"))?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id(namespace.urn("Building", "remote")?)])
            .build()?])
        .property("label", language_property_value(&[("fr", "Source principale"), ("nl", "Hoofdbron")]))
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
    expect_that!(vary(headed.head.headers()), contains(eq(&HeaderName::from_static("accept-language"))));
    let registrations = headed.outcome.items();
    expect_that!(registrations, len(eq(1)));
    expect_that!(registrations, nth(0, has_id(&registration_id)));
    let label_name = "label".parse::<ContextSourcePropertyName>()?;
    let Some(Value::Object(label)) = registrations.first().and_then(|registration| registration.properties().get(&label_name)) else {
        return fail!("the registration holds no label object");
    };
    expect_that!(label.get("type"), some(eq(&Value::from("Property"))));
    expect_that!(label.get("value"), some(eq(&Value::from("Source principale"))));
    expect_that!(label.get("lang"), some(eq(&Value::from("fr"))));
    expect_that!(label.get("languageMap"), none());
    Ok(())
}
