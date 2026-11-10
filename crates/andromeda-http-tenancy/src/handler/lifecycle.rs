//! `PATCH /ngsi-ld/v1/tenants/{tenantName}`: suspend and resume a tenant.

use crate::{
    error::{TenancyHttpError, classify},
    handler::{nonexistent_tenant, parse_tenant_name},
    state::TenancyRouteState,
};
use andromeda_failure_classification::classified_report::ClassifiedReport;
use andromeda_http_extraction::json_payload::JsonPayload;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_tenancy::status::TenantStatus;
use axum::{
    extract::{OriginalUri, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;

/// The writable body of a tenant update.
///
/// Only `isActive` is writable (clause 5.2.12-1). A client-supplied `status` is
/// read-only and ignored on input (clause 5.2.12-2), which is achieved simply by
/// not declaring it: unknown members are dropped rather than rejected.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantPatch {
    /// The requested administrative state: `true` resumes, `false` suspends.
    pub is_active: Option<bool>,
}

/// Suspend or resume a tenant by its `isActive` toggle.
///
/// `false` suspends the tenant: its persisted state flips to `Suspended` and
/// its runtime is evicted at once, so subsequent data requests naming it answer
/// 404 `NonexistentTenant` until it is resumed. `true` resumes it, leaving the
/// next access to cold-load it. Returns 204 with no body.
///
/// # Errors
///
/// Returns a [`ProblemResponse`]: `InvalidRequest` for a malformed name,
/// `BadRequestData` when `isActive` is absent, 404 `NonexistentTenant` for an
/// absent tenant, and the classified failure of the status write.
pub async fn patch(
    State(state): State<TenancyRouteState>,
    OriginalUri(uri): OriginalUri,
    Path(tenant_name): Path<String>,
    JsonPayload(body): JsonPayload<TenantPatch>,
) -> Result<Response, ProblemResponse> {
    let name = parse_tenant_name(tenant_name, &uri)?;
    let Some(is_active) = body.is_active else {
        return Err(ProblemResponse::classified(
            ClassifiedReport::public(
                NgsiLdErrorType::BadRequestData,
                // Clause 5.2.12-1 makes `isActive` the sole writable member; the
                // citation stays here rather than in the client-facing detail.
                "a tenant update must set the writable 'isActive' member",
            ),
            uri.clone(),
        ));
    };
    let status = if is_active { TenantStatus::Active } else { TenantStatus::Suspended };
    match state
        .administration()
        .set_admin_status(&name, status)
        .await
        .map_err(|error| ProblemResponse::classified(classify(&TenancyHttpError::from(error)), uri.clone()))?
    {
        Some(_view) => Ok(StatusCode::NO_CONTENT.into_response()),
        None => Err(nonexistent_tenant(&name, &uri)),
    }
}
