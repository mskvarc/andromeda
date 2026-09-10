//! Retrieve Context Source Identity operation (clauses 5.15.1 and 6.33.3.1).
//!
//! Clause 5.15.1.4 lets an implementation that does not support the operation
//! answer `NotImplemented`; Andromeda always knows its identity, so that branch
//! is unreachable by construction and the handler has no failure of its own
//! beyond rendering.

use crate::{
    error::{SourceIdentityError, classify},
    state::SourceIdentityState,
};
use andromeda_http_extraction::{accept::AcceptFormatNoGeo, context::ContextUrl, prefer::PreferHeader, tenant::TenantHeader};
use andromeda_http_response::{problem_response::ProblemResponse, render::start_rendering, single::NgsiLdResponse};
use andromeda_ngsi_ld_render::response::ResponsePayload;
use andromeda_tenancy::tenant_ref::TenantRef;
use axum::{
    extract::{OriginalUri, State},
    response::{IntoResponse, Response},
};
use chrono::Utc;

/// Retrieve the configured context source identity for the addressed tenant.
///
/// Clause 5.15.1.1 lets a client "obtain identify information about a specific
/// Tenant within a Context Source", and Table 5.2.40-1 makes
/// `contextSourceAlias` that tenant-specific member. The alias is a pure
/// function of the broker's configuration and the tenant the `NGSILD-Tenant`
/// header named, so the tenant is read from the header alone and never
/// resolved against the tenant registry: clause 5.15.1.4 gives this operation
/// no failure that depends on a tenant existing, and resolving one would
/// provision a database for an informational read.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when the typed identity cannot be serialized
/// for the renderer, or when the negotiated representation cannot be rendered.
pub async fn get(
    State(state): State<SourceIdentityState>,
    OriginalUri(uri): OriginalUri,
    tenant: TenantHeader,
    accept: AcceptFormatNoGeo,
    context: ContextUrl,
) -> Result<Response, ProblemResponse> {
    let tenant = TenantRef::from_header(tenant.into_inner());
    let value = serde_json::to_value(state.identity_at(&tenant, Utc::now()))
        .map_err(SourceIdentityError::from)
        .map_err(|error| classify(&error).with_instance(uri.clone()))?;
    let context = context.into_inner();
    let rendered = start_rendering(accept.media_type(), &context, PreferHeader::default().rendering_preferences())
        .with_payload(ResponsePayload::Single(value))
        .render()
        .map_err(SourceIdentityError::from)
        .map_err(|error| classify(&error).with_instance(uri))?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}
