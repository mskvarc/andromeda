//! The health document and its components keep their documented members.

use andromeda_test_suite::{
    broker,
    config::{BrokerRole, SuiteConfig},
    health_check::{HealthDocument, IndicatorName},
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: GET /readyz on the management listener answers a `HealthCheck` holding only id, type and `healthCheck`,
/// every component of which carries a string status and a details object, the `PostgreSQL` details holding only the
/// numbers `poolSize`, `idleConnections` and `pingLatencyMs`
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: the readiness probe and its `HealthCheck` body
#[gtest]
#[tokio::test]
async fn a_obs_rdy_9001_the_health_document_and_its_components_keep_their_documented_members() -> Result<()> {
    let management = broker::client(BrokerRole::FrontManagement)?;
    // The readiness probe sits at the root of the management listener, outside the API root.
    let probe = SuiteConfig::get()?.broker(BrokerRole::FrontManagement)?.join("readyz")?;
    let answer = management
        .send_unchecked(request_to(&management, Method::GET, "")?.uri(probe.as_str().parse()?))
        .await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    // The document type holds only id, type and healthCheck, and an indicator reads only with a string status and an
    // object of details.
    let health: HealthDocument = serde_json::from_slice(answer.body())?;
    let postgresql = health.health_check.components.get(&IndicatorName::Postgresql).or_fail()?;
    expect_that!(postgresql.postgresql_details(), ok(anything()));
    Ok(())
}
