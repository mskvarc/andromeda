//! Generic extraction of tenant-scoped focused state.

use crate::tenant::TenantHeader;
use andromeda_http_response::{problem_response::ProblemResponse, tenant_state_rejection::TenantStateRejection};
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, tenant_ref::TenantRef};
use axum::{extract::FromRequestParts, http::request::Parts};
use futures::future::BoxFuture;

/// Resolves one binding-specific state value for a typed tenant scope.
pub trait TenantStateResolver<T>: Send + Sync {
    /// Resolve or provision `tenant` according to `creation_policy` and classify any failure at the caller's HTTP boundary.
    fn resolve_tenant<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
        instance: &'a axum::http::Uri,
    ) -> BoxFuture<'a, Result<T, ProblemResponse>>;
}

/// Extracted tenant state for operations that require an existing tenant.
pub struct ExistingTenantState<T>(pub T);

/// Extracted tenant state for operations permitted to provision a tenant on demand.
pub struct ProvisionableTenantState<T>(pub T);

/// Resolve the typed tenant header and focused route state under one creation policy.
async fn resolve<S, T>(parts: &mut Parts, state: &S, creation_policy: TenantCreationPolicy) -> Result<T, TenantStateRejection>
where
    S: TenantStateResolver<T>,
{
    let tenant = TenantHeader::from_request_parts(parts, state).await.map_err(TenantStateRejection::Extraction)?;
    let tenant_ref = TenantRef::from_header(tenant.into_inner());
    state
        .resolve_tenant(&tenant_ref, creation_policy, &parts.uri)
        .await
        .map_err(TenantStateRejection::Resolution)
}

impl<S, T> FromRequestParts<S> for ExistingTenantState<T>
where
    S: TenantStateResolver<T>,
{
    type Rejection = TenantStateRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<ExistingTenantState<T>, TenantStateRejection> {
        resolve(parts, state, TenantCreationPolicy::MustExist).await.map(ExistingTenantState)
    }
}

impl<S, T> FromRequestParts<S> for ProvisionableTenantState<T>
where
    S: TenantStateResolver<T>,
{
    type Rejection = TenantStateRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<ProvisionableTenantState<T>, TenantStateRejection> {
        resolve(parts, state, TenantCreationPolicy::CreateOnDemand).await.map(ProvisionableTenantState)
    }
}
