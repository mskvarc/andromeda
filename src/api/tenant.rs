use crate::http::extractor::tenant::TenantHeader;
use andromeda_ngsi_ld_http_headers::ngsild_tenant::NgsiLdTenant;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, tenant_ref::TenantRef};
use axum::{
    body::Body,
    extract::FromRequestParts,
    http::{Request, Response, request::Parts},
    middleware::Next,
};
use headers::{Header, HeaderMapExt};

use crate::{
    api::state::{AndromedaState, BrokerState},
    error::{AndromedaError, ResultInstanceExt},
    http::error::problem::NgsiLdError,
};

/// Extracts tenant state for requests that require the tenant to already exist.
pub struct TenantState(pub AndromedaState);

/// Extracts tenant state, creating the tenant database on demand if it does not exist.
pub struct TenantStateOrCreate(pub AndromedaState);

async fn resolve(parts: &mut Parts, broker: &BrokerState, policy: TenantCreationPolicy) -> Result<AndromedaState, NgsiLdError> {
    let tenant = TenantHeader::from_request_parts(parts, broker)
        .await
        .map_err(|_| NgsiLdError::from(AndromedaError::entity_validation("Invalid NGSILD-Tenant header").with_instance(&parts.uri)))?;
    let tenant_ref = TenantRef::from_header(tenant.into_inner());
    let runtime = broker
        .tenants
        .resolve_state(&tenant_ref, policy)
        .await
        .map_err(AndromedaError::from)
        .with_instance(&parts.uri)?;
    Ok((*runtime).clone())
}

impl FromRequestParts<BrokerState> for TenantState {
    type Rejection = NgsiLdError;

    async fn from_request_parts(parts: &mut Parts, broker: &BrokerState) -> Result<Self, Self::Rejection> {
        let state = resolve(parts, broker, TenantCreationPolicy::MustExist).await?;
        Ok(Self(state))
    }
}

impl FromRequestParts<BrokerState> for TenantStateOrCreate {
    type Rejection = NgsiLdError;

    async fn from_request_parts(parts: &mut Parts, broker: &BrokerState) -> Result<Self, Self::Rejection> {
        let state = resolve(parts, broker, TenantCreationPolicy::CreateOnDemand).await?;
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
