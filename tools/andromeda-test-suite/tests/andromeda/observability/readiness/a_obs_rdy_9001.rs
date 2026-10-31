//! The health document and its components keep their documented members.

use andromeda_test_suite::{
    broker,
    health_check::{HealthDocument, IndicatorName},
    raw_call::call,
};
use googletest::prelude::*;
use http::{Method, StatusCode};

/// Pins: GET /ngsi-ld/v1/health answers a `HealthCheck` holding only id, type and `healthCheck`, every component of
/// which carries a string status and a details object, the `PostgreSQL` details holding only the numbers
/// `poolSize`, `idleConnections` and `pingLatencyMs`
/// Clause: -
/// Recorded: Andromeda extension, not v1.9.1: the health endpoint and its `HealthCheck` body
#[gtest]
#[tokio::test]
async fn a_obs_rdy_9001_the_health_document_and_its_components_keep_their_documented_members() -> Result<()> {
    let front = broker::front()?;
    // The health report is an Andromeda extension no NGSI-LD operation covers, so the request is sent as written.
    let answer = call(&front, Method::GET, "health", &[], None).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    // The document type holds only id, type and healthCheck, and an indicator reads only with a string status and an
    // object of details.
    let health: HealthDocument = serde_json::from_slice(answer.body())?;
    let postgresql = health.health_check.components.get(&IndicatorName::Postgresql).or_fail()?;
    expect_that!(postgresql.postgresql_details(), ok(anything()));
    Ok(())
}
