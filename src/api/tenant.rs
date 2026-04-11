use crate::http::{extractor::tenant::TenantHeader, header::ngsild_tenant::NgsiLdTenant};
use axum::{
    body::Body,
    extract::FromRequestParts,
    http::{Request, Response, request::Parts},
    middleware::Next,
};
use headers::{Header, HeaderMapExt};

use crate::{
    api::state::{AndromedaState, AppState},
    http::error::problem::NgsiLdError,
    tenancy::types::{TenantCreationPolicy, TenantRef},
};

/// Extracts tenant state for requests that require the tenant to already exist.
pub struct TenantState(pub AndromedaState);

/// Extracts tenant state, creating the tenant database on demand if it does not exist.
pub struct TenantStateOrCreate(pub AndromedaState);

async fn resolve(parts: &mut Parts, app: &AppState, policy: TenantCreationPolicy) -> Result<AndromedaState, NgsiLdError> {
    let tenant = TenantHeader::from_request_parts(parts, app).await.map_err(|_| {
        NgsiLdError::from(crate::core::error::AndromedaError::entity_validation("Invalid NGSILD-Tenant header").with_instance(parts.uri.clone()))
    })?;
    let tenant_ref = TenantRef::from_header(tenant.into_inner());
    let runtime = app
        .tenants
        .resolve_state(&tenant_ref, policy)
        .await
        .map_err(|e| e.with_instance(parts.uri.clone()))?;
    Ok((*runtime).clone())
}

impl FromRequestParts<AppState> for TenantState {
    type Rejection = NgsiLdError;

    async fn from_request_parts(parts: &mut Parts, app: &AppState) -> Result<Self, Self::Rejection> {
        let state = resolve(parts, app, TenantCreationPolicy::MustExist).await?;
        Ok(Self(state))
    }
}

impl FromRequestParts<AppState> for TenantStateOrCreate {
    type Rejection = NgsiLdError;

    async fn from_request_parts(parts: &mut Parts, app: &AppState) -> Result<Self, Self::Rejection> {
        let state = resolve(parts, app, TenantCreationPolicy::CreateOnDemand).await?;
        Ok(Self(state))
    }
}

pub async fn echo_tenant_header_middleware(request: Request<Body>, next: Next) -> Response<Body> {
    let tenant: Option<NgsiLdTenant> = {
        let mut values = request.headers().get_all(NgsiLdTenant::name()).iter();
        headers::Header::decode(&mut values).ok()
    };

    let mut response = next.run(request).await;

    if let Some(tenant) = tenant {
        response.headers_mut().typed_insert(tenant);
    }

    response
}
