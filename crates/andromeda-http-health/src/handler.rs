//! Health report operation.

use crate::{
    error::{HealthHttpError, classify},
    state::HealthRouteState,
};
use andromeda_health::{
    report::{HealthCheckResponse, HealthCheckType},
    status::HealthStatus,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::NGSI_LD_URN_NID};
use axum::{
    Json,
    extract::{OriginalUri, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Serialize;
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// The body of a liveness probe: the process is up and answering.
#[derive(Debug, Serialize)]
pub struct LivenessReport {
    /// Always [`HealthStatus::Up`]; liveness consults no dependency.
    status: HealthStatus,
}

/// Report that the process is up, without probing any dependency.
///
/// Liveness answers only "is the process running and able to serve"; readiness
/// (`/ready`, which reuses [`check`]) is the signal that weighs dependencies.
pub async fn live() -> impl IntoResponse {
    (StatusCode::OK, Json(LivenessReport { status: HealthStatus::Up }))
}

/// Return aggregate broker health and component details.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the health-check response identifier
/// cannot be built as an NGSI-LD URN.
pub async fn check(State(state): State<HealthRouteState>, OriginalUri(uri): OriginalUri) -> Result<impl IntoResponse, ProblemResponse> {
    let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("HealthCheck:{}", Uuid::new_v4()))
        .build()
        .map_err(HealthHttpError::from)
        .and_then(|urn| Identifier::try_from(urn).map_err(HealthHttpError::from))
        .map_err(|error| classify(&error).with_instance(uri))?;
    let health_check = state.health().check_all().await;
    let status = match health_check.status {
        HealthStatus::Up | HealthStatus::Degraded => StatusCode::OK,
        HealthStatus::Down => StatusCode::SERVICE_UNAVAILABLE,
    };
    Ok((
        status,
        Json(HealthCheckResponse {
            id,
            type_: HealthCheckType,
            health_check,
        }),
    ))
}
