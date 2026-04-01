use crate::api::extractor::tenant::{NGSILD_TENANT_HEADER, TenantHeader};
use axum::{
    body::Body,
    extract::FromRequestParts,
    http::{HeaderValue, Request, Response, request::Parts},
    middleware::Next,
};

use crate::{
    api::state::{AndromedaState, AppState},
    error::api::ApiError,
    tenancy::TenantRef,
};

pub struct TenantState(pub AndromedaState);
pub struct TenantStateOrCreate(pub AndromedaState);

async fn resolve(parts: &mut Parts, app: &AppState, allow_implicit_creation: bool) -> Result<AndromedaState, ApiError> {
    let tenant = TenantHeader::from_request_parts(parts, app).await.map_err(|_| {
        ApiError::from_error(
            crate::error::AndromedaError::bad_request_data("Invalid NGSILD-Tenant header"),
            parts.uri.clone(),
        )
    })?;
    let tenant_ref = TenantRef::from_header(tenant.into_inner());
    let runtime = app
        .tenants
        .resolve_state(&tenant_ref, allow_implicit_creation)
        .await
        .map_err(|e| ApiError::from_error(e, parts.uri.clone()))?;
    Ok((*runtime).clone())
}

impl FromRequestParts<AppState> for TenantState {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &AppState) -> Result<Self, Self::Rejection> {
        let state = resolve(parts, app, false).await?;
        Ok(Self(state))
    }
}

impl FromRequestParts<AppState> for TenantStateOrCreate {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &AppState) -> Result<Self, Self::Rejection> {
        let state = resolve(parts, app, true).await?;
        Ok(Self(state))
    }
}

pub async fn echo_tenant_header_middleware(request: Request<Body>, next: Next) -> Response<Body> {
    let tenant = request.headers().get(NGSILD_TENANT_HEADER).and_then(|v| v.to_str().ok()).map(str::to_string);

    let mut response = next.run(request).await;

    if let Some(value) = tenant
        && let Ok(header) = HeaderValue::from_str(&value)
    {
        response.headers_mut().insert(NGSILD_TENANT_HEADER, header);
    }

    response
}
