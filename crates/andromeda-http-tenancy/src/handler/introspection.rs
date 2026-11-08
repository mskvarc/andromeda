//! `GET /ngsi-ld/v1/tenants/{tenantName}`: the per-tenant introspection resource.

use crate::{
    error::{TenancyHttpError, classify},
    handler::{nonexistent_tenant, parse_tenant_name},
    options::TenantOptions,
    params::TenantIntrospectionParams,
    state::TenancyRouteState,
    tenant_response::build_tenant,
};
use andromeda_http_extraction::query::ValidatedQuery;
use andromeda_http_response::problem_response::ProblemResponse;
use axum::{
    Json,
    extract::{OriginalUri, Path, State},
    response::{IntoResponse, Response},
};

/// Return one tenant's resource.
///
/// Cheap by default: a registry row and an in-memory residency read.
/// `?options=sysAttrs` adds the system timestamps and `?options=usage` the
/// operational block (which queries the tenant database). An unknown tenant is
/// answered with 404 `NonexistentTenant` on the management path, reporting its
/// real status only for one that exists.
///
/// # Errors
///
/// Returns a [`ProblemResponse`]: `InvalidRequest` for a bad name or `options`
/// keyword, 404 `NonexistentTenant` for an absent tenant, and the classified
/// failure of a registry or usage read.
pub async fn get(
    State(state): State<TenancyRouteState>,
    OriginalUri(uri): OriginalUri,
    Path(tenant_name): Path<String>,
    ValidatedQuery(params): ValidatedQuery<TenantIntrospectionParams>,
) -> Result<Response, ProblemResponse> {
    let options = TenantOptions::parse(params.options.as_deref()).map_err(|error| error.classify().with_instance(uri.clone()))?;
    let name = parse_tenant_name(tenant_name, &uri)?;
    let view = state
        .administration()
        .tenant(&name)
        .await
        .map_err(|error| classify(&TenancyHttpError::from(error)).with_instance(uri.clone()))?;
    match view {
        Some(view) => {
            let tenant = build_tenant(&view, options, state.usage())
                .await
                .map_err(|error| classify(&error).with_instance(uri.clone()))?;
            Ok(Json(tenant).into_response())
        }
        None => Err(nonexistent_tenant(&name, &uri)),
    }
}
