//! A database that stops answering fails readiness within the probe's bound, and readiness returns once it answers.

use andromeda_test_suite::{
    config::BrokerRole,
    database_proxy::DatabaseProxy,
    eventually::{Poll, delivery_deadline, eventually},
    health_check::{AggregateStatus, IndicatorName},
    readiness_probe::ReadinessProbe,
};
use googletest::prelude::*;
use http::StatusCode;
use std::time::Duration;

/// The longest the probe may take to answer once the database stops answering: the one second the ping waits, and a
/// second for the request around it, well inside the five seconds of the broker's `database.acquire_timeout`.
const ANSWER_BOUND: Duration = Duration::from_secs(2);

/// Pins: once the broker's connections to its PostgreSQL server fail and every new one is refused, GET /readyz on the
/// management listener answers 503 within two seconds, with its `postgresql` component down and its details saying
/// why; once connections are carried again, it answers 200
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: the readiness probe and its `HealthCheck` body
#[gtest]
#[tokio::test]
async fn a_obs_rdy_9004_a_database_that_stops_answering_fails_readiness_within_the_probes_bound() -> Result<()> {
    let probe = ReadinessProbe::of(BrokerRole::OutageManagement)?;
    let proxy = DatabaseProxy::of()?;
    let probe = &probe;
    proxy.cut().await?;
    let down = eventually(delivery_deadline(), || async move {
        match probe.ask().await {
            Ok(reading) if reading.status == StatusCode::SERVICE_UNAVAILABLE => Poll::Ready(reading),
            Ok(reading) => Poll::Pending(format!("/readyz answered {} after the database stopped answering", reading.status)),
            Err(failure) => Poll::Pending(format!("{failure:?}")),
        }
    })
    .await;
    proxy.restore().await?;
    let down = down?;
    expect_that!(down.took, lt(ANSWER_BOUND));
    expect_that!(down.document.health_check.status, eq(AggregateStatus::Down));
    let database = down.document.health_check.components.get(&IndicatorName::Postgresql).or_fail()?;
    expect_that!(database.status, eq("down"));
    expect_that!(database.postgresql_failure().map(|failure| failure.error), ok(not(eq(""))));
    let ready = eventually(delivery_deadline(), || async move {
        match probe.ask().await {
            Ok(reading) if reading.status == StatusCode::OK => Poll::Ready(reading.status),
            Ok(reading) => Poll::Pending(format!("/readyz answered {} after the database answered again", reading.status)),
            Err(failure) => Poll::Pending(format!("{failure:?}")),
        }
    })
    .await?;
    verify_that!(ready, eq(StatusCode::OK))
}
