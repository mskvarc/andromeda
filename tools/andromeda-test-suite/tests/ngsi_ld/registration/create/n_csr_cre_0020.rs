//! Observation and management intervals read back.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{DateTime, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo, TimeInterval},
};

/// Pins: Create Context Source Registration with an `observationInterval` holding only `startAt` and a
/// `managementInterval` holding `startAt` and `endAt`, and Retrieve Context Source Registration answers both with
/// the same instants and no `endAt` on the first
/// Clause: 5.2.9, 5.2.11
#[gtest]
#[tokio::test]
async fn n_csr_cre_0020_observation_and_management_intervals_read_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([building_type.as_str()]).build()?])
        .observation_interval(TimeInterval::starting_at("2026-01-01T00:00:00Z".parse()?))
        .management_interval(TimeInterval::between("2026-01-01T00:00:00Z".parse()?, "2026-02-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(
        registration_2.observation_interval().map(TimeInterval::start_at).map(DateTime::timestamp),
        some(eq("2026-01-01T00:00:00Z".parse::<DateTime>()?.timestamp()))
    );
    expect_that!(registration_2.observation_interval().and_then(TimeInterval::end_at), none());
    expect_that!(
        registration_2.management_interval().map(TimeInterval::start_at).map(DateTime::timestamp),
        some(eq("2026-01-01T00:00:00Z".parse::<DateTime>()?.timestamp()))
    );
    expect_that!(
        registration_2.management_interval().and_then(TimeInterval::end_at).map(DateTime::timestamp),
        some(eq("2026-02-01T00:00:00Z".parse::<DateTime>()?.timestamp()))
    );
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
