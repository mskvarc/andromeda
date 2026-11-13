//! A platform pool with no connection to spare leaves the broker ready and shows in the database indicator's details.

use andromeda_postgresql_schema::tenancy::Tenants;
use andromeda_postgresql_seaquery::relation_lock::{LockMode, RelationLock};
use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    eventually::{Poll, delivery_deadline, eventually},
    health_check::{AggregateStatus, HealthDocument, IndicatorName},
    load_outcome::LoadOutcome,
    namespace::Namespace,
    readiness_probe::ReadinessProbe,
    relation_hold::RelationHold,
};
use googletest::prelude::*;
use http::StatusCode;
use sea_query::TableName;
use std::time::Duration;

/// The longest the probe may take to answer while the database answers, however busy the platform pool is.
const ANSWER_BOUND: Duration = Duration::from_secs(1);

/// Requests for tenants the broker has not loaded. Each reads the held registry over a connection of the platform
/// pool and waits there, so two hold the broker's two platform connections and the third waits for one.
const LOADS: usize = 3;

/// Whether the `postgresql` component of `document` reports the platform pool's every connection open and in use.
fn platform_pool_saturated(document: &HealthDocument) -> bool {
    document
        .health_check
        .components
        .get(&IndicatorName::Postgresql)
        .and_then(|database| database.postgresql_details().ok())
        .is_some_and(|details| details.platform_pool_saturated())
}

/// Pins: while every connection of the platform pool waits on the tenant registry, GET /readyz on the management
/// listener answers 200 within a second, with its `postgresql` component up and its details showing every connection
/// of the platform pool open and none idle
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: the readiness probe and its `HealthCheck` body
#[gtest]
#[tokio::test]
async fn a_obs_rdy_9003_a_platform_pool_with_no_connection_to_spare_leaves_the_broker_ready() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::client(BrokerRole::PlatformPool)?;
    let probe = ReadinessProbe::of(BrokerRole::PlatformPoolManagement)?;
    let registry = RelationHold::seize(
        BrokerRole::PlatformPool,
        RelationLock::waiting(vec![TableName::from(Tenants::Table)], LockMode::AccessExclusive),
    )
    .await?;
    let loads = (0..LOADS)
        .map(|index| {
            let tenant = broker.with_tenant(namespace.numbered_tenant(index)?).with_responses();
            let id = namespace.urn("Building", index)?;
            Ok(tokio::spawn(async move { LoadOutcome::of(&tenant.entities().retrieve(&id).send().await) }))
        })
        .collect::<Result<Vec<_>>>()?;
    let probe = &probe;
    let reading = eventually(delivery_deadline(), || async move {
        match probe.ask().await {
            Ok(reading) if reading.status != StatusCode::OK || platform_pool_saturated(&reading.document) => Poll::Ready(reading),
            Ok(reading) => Poll::Pending(format!("/readyz answered {} before the platform pool was saturated", reading.status)),
            Err(failure) => Poll::Pending(format!("{failure:?}")),
        }
    })
    .await;
    registry.release().await?;
    let reading = reading?;
    expect_that!(reading.status, eq(StatusCode::OK));
    expect_that!(reading.took, lt(ANSWER_BOUND));
    expect_that!(
        reading.document.health_check.status,
        any!(eq(AggregateStatus::Up), eq(AggregateStatus::Degraded))
    );
    let database = reading.document.health_check.components.get(&IndicatorName::Postgresql).or_fail()?;
    expect_that!(database.status, eq("up"));
    expect_that!(platform_pool_saturated(&reading.document), eq(true));
    for load in loads {
        expect_that!(load.await?.status(), some(anything()));
    }
    Ok(())
}
