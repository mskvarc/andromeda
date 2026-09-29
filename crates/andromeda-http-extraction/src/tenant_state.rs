//! Generic extraction of tenant-scoped focused state.

use crate::{addressed_uri::addressed_uri, tenant::TenantHeader};
use andromeda_http_response::{problem_response::ProblemResponse, tenant_state_rejection::TenantStateRejection};
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, tenant_ref::TenantRef};
use axum::{
    extract::FromRequestParts,
    http::{Uri, request::Parts},
};
use futures::future::BoxFuture;

/// Resolves one binding-specific state value for a typed tenant scope.
pub trait TenantStateResolver<T>: Send + Sync {
    /// Resolve or provision `tenant` according to `creation_policy` and classify any failure at the caller's HTTP boundary.
    fn resolve_tenant<'a>(
        &'a self,
        tenant: &'a TenantRef,
        creation_policy: TenantCreationPolicy,
        instance: &'a Uri,
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
        .resolve_tenant(&tenant_ref, creation_policy, &addressed_uri(parts))
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

#[cfg(test)]
mod tests {
    use super::{ExistingTenantState, TenantStateResolver};
    use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use andromeda_tenancy::{creation_policy::TenantCreationPolicy, tenant_ref::TenantRef};
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode, Uri},
        routing::get,
    };
    use futures::future::BoxFuture;
    use std::{
        error::Error,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use tower::ServiceExt;

    /// Capability whose handler invocation is observable.
    struct HandlerCapability;

    /// Number of times the test handler has executed.
    static HANDLER_CALLS: AtomicUsize = AtomicUsize::new(0);

    /// Resolver that rejects before yielding its capability.
    #[derive(Clone)]
    struct RejectingResolver;

    impl TenantStateResolver<HandlerCapability> for RejectingResolver {
        fn resolve_tenant<'a>(
            &'a self,
            _tenant: &'a TenantRef,
            _creation_policy: TenantCreationPolicy,
            instance: &'a Uri,
        ) -> BoxFuture<'a, Result<HandlerCapability, ProblemResponse>> {
            Box::pin(
                async move { Err(ClassifiedReport::public(NgsiLdErrorType::NonexistentTenant, "Tenant is not registered").with_instance(instance.clone())) },
            )
        }
    }

    /// Increment the observable handler count when tenant resolution succeeds.
    async fn counted_handler(ExistingTenantState(_capability): ExistingTenantState<HandlerCapability>) -> StatusCode {
        HANDLER_CALLS.fetch_add(1, Ordering::SeqCst);
        StatusCode::OK
    }

    #[tokio::test]
    async fn tenant_resolution_rejection_is_typed_and_prevents_handler_execution() -> Result<(), Box<dyn Error>> {
        HANDLER_CALLS.store(0, Ordering::SeqCst);
        let router = Router::new().route("/entities", get(counted_handler)).with_state(RejectingResolver);
        let response = router
            .oneshot(Request::builder().uri("/entities").header("NGSILD-Tenant", "missing").body(Body::empty())?)
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_FOUND);
        pretty_assertions::assert_eq!(HANDLER_CALLS.load(Ordering::SeqCst), 0);
        Ok(())
    }
}
