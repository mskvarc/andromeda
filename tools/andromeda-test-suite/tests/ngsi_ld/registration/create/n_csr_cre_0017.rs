//! A zero refresh rate and cache duration read back as written.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{Duration, NgsiLdMediaType, RawJson},
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Create Context Source Registration accepts the zero duration PT0S as `refreshRate` and as management
/// `cacheDuration`, and Retrieve Context Source Registration answers each as written
/// Clause: 5.9.2.4, Table 5.2.9-1, Table 5.2.34-1
#[gtest]
#[tokio::test]
async fn n_csr_cre_0017_a_zero_refresh_rate_and_cache_duration_read_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_zero_refresh_rate_id = namespace.registration_id("zero-refresh-rate")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_zero_cache_duration_id = namespace.registration_id("zero-cache-duration")?;
    let registration_zero_refresh_rate = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_zero_refresh_rate_id.clone())
        .information([RegistrationInfo::builder().entities([building_type.as_str()]).build()?])
        .build()?;
    // The model refuses it (a registration refreshRate), so `refreshRate` is written as text.
    let refresh_rate: RawJson = r#""PT0S""#.parse()?;
    let document = UncheckedDocument::try_from(registration_zero_refresh_rate.render())?.set_member("refreshRate", &refresh_rate);
    let request = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_zero_refresh_rate,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_zero_refresh_rate_cleanup = Cleanup::of(&broker).registration(&registration_zero_refresh_rate_id);
    let registration = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_zero_refresh_rate_id)
        .send()
        .await?;
    expect_that!(registration.refresh_rate(), some(eq(&"PT0S".parse::<Duration>()?)));
    broker.registrations().delete(&registration_zero_refresh_rate_id).send().await?;
    let registration_zero_cache_duration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_zero_cache_duration_id.clone())
        .information([RegistrationInfo::builder().entities([building_type.as_str()]).build()?])
        .build()?;
    // The model refuses it (a registration management), so `management` is written as text.
    let management: RawJson = r#"{"cacheDuration": "PT0S"}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_zero_cache_duration.render())?.set_member("management", &management);
    let request_2 = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_zero_cache_duration,
        })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let _registration_zero_cache_duration_cleanup = Cleanup::of(&broker).registration(&registration_zero_cache_duration_id);
    let registration_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_zero_cache_duration_id)
        .send()
        .await?;
    expect_that!(
        registration_2.management().and_then(|management| management.cache_duration()),
        some(eq(&"PT0S".parse::<Duration>()?))
    );
    broker.registrations().delete(&registration_zero_cache_duration_id).send().await?;
    Ok(())
}
