//! A zero refresh rate and cache duration replace and read back as written.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::answered, registration_update_request::update_without_type};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Duration, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, RegistrationFragment, RegistrationInfo, RegistrationManagementInfo},
};

/// Pins: Update Context Source Registration replacing `refreshRate` and management `cacheDuration` with the zero
/// duration PT0S answers 204, and Retrieve Context Source Registration answers each as written
/// Clause: 5.9.3.4, Table 5.2.9-1, Table 5.2.34-1
#[gtest]
#[tokio::test]
async fn n_csr_upd_9002_a_zero_refresh_rate_and_cache_duration_replace_and_read_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id("zero-durations")?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(building_type.clone())]).build()?])
        .refresh_rate("PT1H".parse::<Duration>()?)
        .management(RegistrationManagementInfo::builder().cache_duration("PT10S".parse::<Duration>()?).build()?)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let fragment = RegistrationFragment::builder()
        .refresh_rate("PT0S".parse::<Duration>()?)
        .management(RegistrationManagementInfo::builder().cache_duration("PT0S".parse::<Duration>()?).build()?)
        .build()?;
    let request = update_without_type(&broker, &registration_id, &fragment)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.refresh_rate().map(ToString::to_string), some(eq("PT0S")));
    expect_that!(
        retrieved
            .management()
            .and_then(RegistrationManagementInfo::cache_duration)
            .map(ToString::to_string),
        some(eq("PT0S"))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
