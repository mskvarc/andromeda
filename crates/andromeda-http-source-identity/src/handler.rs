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
use andromeda_http_context_registry::response_rendering::ResponseContextSource;
use andromeda_http_extraction::{
    accept::AcceptFormatNoGeo,
    api_root::ApiRoot,
    classification::classify_tenancy_error,
    context::ContextUrl,
    prefer::PreferHeader,
    query::ValidatedQuery,
    tenant::TenantHeader,
};
use andromeda_http_response::{problem_response::ProblemResponse, rendered_response::RenderedResponse, single::NgsiLdResponse};
use andromeda_ngsi_ld_request::empty::EmptyParams;
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
/// provision a database for an informational read. The `@context` the
/// response names is resolved and hosted with the contexts of the default
/// tenant instead, which clause 4.14 says always exists.
///
/// # Errors
///
/// Returns a [`ProblemResponse`] when a document the request context names
/// cannot be loaded or its wrapper cannot be hosted, or when the identity
/// cannot be written in the negotiated representation.
pub async fn get(
    // Clause 6.3.20: the operation defines no parameter, so any is an
    // `InvalidRequest`; it is read first, before anything else is resolved.
    ValidatedQuery(EmptyParams {}): ValidatedQuery<EmptyParams>,
    State(state): State<SourceIdentityState>,
    OriginalUri(uri): OriginalUri,
    api_root: ApiRoot,
    tenant: TenantHeader,
    accept: AcceptFormatNoGeo,
    ContextUrl(context): ContextUrl,
) -> Result<Response, ProblemResponse> {
    let tenant = TenantRef::from_header(tenant.into_inner());
    let identity = state.identity_at(&tenant, Utc::now());
    let default_tenant = state
        .contexts()
        .resolve()
        .await
        .map_err(|error| ProblemResponse::classified(classify_tenancy_error(&error), uri.clone()))?;
    let ready = ResponseContextSource::new(&default_tenant.engine, &context, &default_tenant.context_hosting, &api_root)
        .start_rendering_normalized(accept.media_type(), PreferHeader::default().rendering_preferences())
        .await
        .map_err(SourceIdentityError::from)
        .map_err(|error| ProblemResponse::classified(classify(&error), uri.clone()))?
        .with_body(&identity);
    let rendered = RenderedResponse::resource(ready)
        .map_err(SourceIdentityError::from)
        .map_err(|error| ProblemResponse::classified(classify(&error), uri))?;
    Ok(NgsiLdResponse::ok(rendered).into_response())
}
