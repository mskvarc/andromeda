//! Health reports the aggregate and each active component.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    health_check::{AggregateStatus, HealthDocument, HealthType, IndicatorName},
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode, header::CONTENT_TYPE};

/// Pins: GET /readyz on the management listener answers a plain JSON `HealthCheck` with no @context, an aggregate
/// status of up or degraded, and only the active indicators, the `PostgreSQL` one up with numeric pool details and the
/// background tasks one up with no stopped task
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: the readiness probe and its `HealthCheck` body
#[gtest]
#[tokio::test]
async fn a_obs_rdy_0001_health_reports_the_aggregate_and_each_active_component() -> Result<()> {
    let management = broker::client(BrokerRole::FrontManagement)?;
    // The readiness probe sits at the root of the management listener, outside the API root.
    let probe = SuiteConfig::get()?.broker(BrokerRole::FrontManagement)?.join("readyz")?;
    let answer = management
        .send_unchecked(request_to(&management, Method::GET, "")?.uri(probe.as_str().parse()?))
        .await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    expect_that!(
        answer.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()),
        some(eq("application/json"))
    );
    // The document type refuses a member beside id, type and healthCheck, `@context` included, and an indicator
    // of any component but backgroundTasks, mqtt and postgresql.
    let health: HealthDocument = serde_json::from_slice(answer.body())?;
    expect_that!(health.id.as_str(), contains_regex("^urn:ngsi-ld:HealthCheck:."));
    expect_that!(health.health_type, eq(HealthType::HealthCheck));
    expect_that!(health.health_check.status, any!(eq(AggregateStatus::Up), eq(AggregateStatus::Degraded)));
    let postgresql = health.health_check.components.get(&IndicatorName::Postgresql).or_fail()?;
    expect_that!(postgresql.status, eq("up"));
    expect_that!(postgresql.postgresql_details(), ok(anything()));
    let tasks = health.health_check.components.get(&IndicatorName::BackgroundTasks).or_fail()?;
    expect_that!(tasks.status, eq("up"));
    expect_that!(tasks.background_task_details().map(|details| details.stopped_tasks), ok(is_empty()));
    Ok(())
}
