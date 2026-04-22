//! Handler for the `/health` endpoint.

use crate::{
    core::error::AndromedaError,
    domain::{
        health::{HealthCheckDetails, HealthCheckResponse, HealthCheckType, HealthStatus},
        uri::{NGSI_LD_URN_NID, Uri},
    },
    health::Health,
    http::error::problem::NgsiLdError,
};
use axum::{Extension, Json, http::StatusCode, response::IntoResponse};
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// GET /ngsi-ld/v1/health -- Check broker health and return component status.
pub async fn check(Extension(health): Extension<Health>) -> Result<impl IntoResponse, NgsiLdError> {
    let id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("HealthCheck:{}", Uuid::new_v4()))
        .build()
        .map_err(AndromedaError::from)?;

    let (status, components) = health.check_all().await;

    let status_code = match status {
        HealthStatus::Up => StatusCode::OK,
        HealthStatus::Down => StatusCode::SERVICE_UNAVAILABLE,
    };

    let response = HealthCheckResponse {
        id: Uri::Urn(id),
        type_: HealthCheckType::HealthCheck,
        health_check: HealthCheckDetails { status, components },
    };

    Ok((status_code, Json(response)))
}
