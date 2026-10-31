//! Health reports the aggregate and each active component.

use andromeda_test_suite::{
    broker,
    health_check::{AggregateStatus, HealthDocument, HealthType, IndicatorName},
    raw_call::call,
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::CONTENT_TYPE};

/// Pins: GET /ngsi-ld/v1/health answers a plain JSON `HealthCheck` with no @context, an aggregate status of up or
/// degraded, and only the active indicators, the `PostgreSQL` one up with numeric pool details
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: the health endpoint and its `HealthCheck` body
#[gtest]
#[tokio::test]
async fn a_obs_rdy_0001_health_reports_the_aggregate_and_each_active_component() -> Result<()> {
    let front = broker::front()?;
    // The health report is an Andromeda extension no NGSI-LD operation covers, so the request is sent as written.
    let answer = call(&front, Method::GET, "health", &[], None).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    expect_that!(
        answer.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
        some(eq("application/json"))
    );
    // The document type refuses a member beside id, type and healthCheck, `@context` included, and an indicator
    // of any component but mqtt and postgresql.
    let health: HealthDocument = serde_json::from_slice(answer.body())?;
    expect_that!(health.id.as_str(), contains_regex("^urn:ngsi-ld:HealthCheck:."));
    expect_that!(health.health_type, eq(HealthType::HealthCheck));
    expect_that!(health.health_check.status, any!(eq(AggregateStatus::Up), eq(AggregateStatus::Degraded)));
    let postgresql = health.health_check.components.get(&IndicatorName::Postgresql).or_fail()?;
    expect_that!(postgresql.status, eq("up"));
    expect_that!(postgresql.postgresql_details(), ok(anything()));
    Ok(())
}
