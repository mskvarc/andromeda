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
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use axum::{
    Json,
    extract::{OriginalUri, State},
    http::StatusCode,
    response::IntoResponse,
};
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// Return aggregate broker health and component details.
pub async fn check(State(state): State<HealthRouteState>, OriginalUri(uri): OriginalUri) -> Result<impl IntoResponse, ProblemResponse> {
    let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("HealthCheck:{}", Uuid::new_v4()))
        .build()
        .map_err(HealthHttpError::from)
        .map_err(|error| classify(&error).with_instance(uri))?;
    let health_check = state.health().check_all().await;
    let status = match health_check.status {
        HealthStatus::Up | HealthStatus::Degraded => StatusCode::OK,
        HealthStatus::Down => StatusCode::SERVICE_UNAVAILABLE,
    };
    Ok((
        status,
        Json(HealthCheckResponse {
            id: Uri::Urn(id),
            type_: HealthCheckType,
            health_check,
        }),
    ))
}
