//! Combined tenant and context-space targeting for scoped NGSI-LD operations.

use crate::{addressed_uri::addressed_uri, snapshot::SnapshotHeader, tenant::TenantHeader, tenant_state::TenantStateResolver};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_response::{
    classified_report::ClassifiedReport,
    internal_failure_summary::InternalFailureSummary,
    problem_response::ProblemResponse,
    tenant_state_rejection::TenantStateRejection,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, tenant_ref::TenantRef};
use axum::{
    extract::FromRequestParts,
    http::{Uri, request::Parts},
};
use futures::future::BoxFuture;
use std::{sync::Arc, time::Duration};
use thiserror::Error;

/// Failure to resolve an `NGSILD-Snapshot` value to an addressable context branch.
#[derive(Debug, Error)]
pub enum ContextTargetResolutionError {
    /// No unexpired Snapshot owns the requested identifier.
    #[error("Snapshot {identifier} was not found")]
    NotFound {
        /// Requested Snapshot URI.
        identifier: Identifier,
    },
    /// Capture or clone promotion exceeded the scoped-operation deadline.
    #[error("Snapshot {identifier} is still preparing")]
    PreparationTimeout {
        /// Requested Snapshot URI.
        identifier: Identifier,
    },
    /// The Snapshot's stored branch is behind this broker and cannot be read
    /// until its upgrade succeeds. The tenant keeps serving; only this
    /// Snapshot is refused, as an internal error that names it.
    #[error("Snapshot {identifier} is quarantined: its stored branch has not been upgraded to this broker's schema")]
    Quarantined {
        /// Requested Snapshot URI.
        identifier: Identifier,
    },
    /// A connection pool was saturated before the Snapshot target could be
    /// resolved. Backpressure, not a fault: shed as 503 with `Retry-After`.
    #[error("the datastore connection pool is saturated; retry shortly")]
    Saturated,
    /// The store holding the Snapshot's control record could not be read.
    #[error("the Snapshot target could not be read: {0}")]
    Persistence(#[source] PersistenceFailure),
    /// Durable target resolution failed internally; the resolver records the
    /// failure for operators where it classifies it.
    #[error("Snapshot target resolution failed")]
    Internal,
}

impl ContextTargetResolutionError {
    /// Convert a target-resolution failure into its request-bound HTTP problem.
    fn into_problem(self, instance: Uri) -> ProblemResponse {
        match self {
            ContextTargetResolutionError::NotFound { .. } => {
                ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, self.to_string()).with_instance(instance)
            }
            ContextTargetResolutionError::PreparationTimeout { .. } => {
                ClassifiedReport::public_binding(ProblemClassification::ServiceUnavailable, self.to_string())
                    .with_retry_after(Duration::from_secs(1))
                    .with_instance(instance)
            }
            ContextTargetResolutionError::Quarantined { .. } => {
                ClassifiedReport::public(NgsiLdErrorType::InternalError, self.to_string()).with_instance(instance)
            }
            ContextTargetResolutionError::Saturated => ClassifiedReport::store_saturated(self.to_string()).with_instance(instance),
            ContextTargetResolutionError::Persistence(_) | ContextTargetResolutionError::Internal => {
                ClassifiedReport::internal(&self, InternalFailureSummary::new("failed to resolve the request's snapshot target")).with_instance(instance)
            }
        }
    }
}

/// Resolves one validated Snapshot URI to its durable branch target.
pub trait SnapshotTargetResolver: Send + Sync {
    /// Await terminal promotion when necessary and return the typed target.
    fn resolve<'a>(&'a self, snapshot_id: &'a Identifier) -> BoxFuture<'a, Result<ContextTarget, ContextTargetResolutionError>>;
}

/// Route-focused state that exposes its live branch and Snapshot resolver.
pub trait ContextTargetProvider: Send + Sync {
    /// Return the tenant's durable live branch.
    fn live_target(&self) -> &ContextTarget;

    /// Return the resolver used only when `NGSILD-Snapshot` is present.
    fn snapshot_target_resolver(&self) -> &dyn SnapshotTargetResolver;
}

impl<T> ContextTargetProvider for Arc<T>
where
    T: ContextTargetProvider,
{
    fn live_target(&self) -> &ContextTarget {
        self.as_ref().live_target()
    }

    fn snapshot_target_resolver(&self) -> &dyn SnapshotTargetResolver {
        self.as_ref().snapshot_target_resolver()
    }
}

/// Existing tenant state paired with the context branch selected at the HTTP boundary.
pub struct ExistingTargetedTenantState<T> {
    /// Focused services for the selected tenant.
    pub services: T,
    /// Live or Snapshot branch selected for this request.
    pub target: ContextTarget,
    /// Validated Snapshot URI to echo on scoped responses.
    pub snapshot: Option<Identifier>,
}

/// Provisionable tenant state paired with the context branch selected at the HTTP boundary.
pub struct ProvisionableTargetedTenantState<T> {
    /// Focused services for the selected tenant.
    pub services: T,
    /// Live or Snapshot branch selected for this request.
    pub target: ContextTarget,
    /// Validated Snapshot URI to echo on scoped responses.
    pub snapshot: Option<Identifier>,
}

/// Resolve tenant state and its live or Snapshot context target as one boundary operation.
async fn resolve<S, T>(
    parts: &mut Parts,
    state: &S,
    live_creation_policy: TenantCreationPolicy,
) -> Result<(T, ContextTarget, Option<Identifier>), TenantStateRejection>
where
    S: TenantStateResolver<T>,
    T: ContextTargetProvider,
{
    let tenant = TenantHeader::from_request_parts(parts, state).await.map_err(TenantStateRejection::Extraction)?;
    let snapshot = SnapshotHeader::from_request_parts(parts, state)
        .await
        .map_err(TenantStateRejection::Extraction)?
        .into_inner();
    let tenant_ref = TenantRef::from_header(tenant.into_inner());
    let creation_policy = if snapshot.is_some() {
        TenantCreationPolicy::MustExist
    } else {
        live_creation_policy
    };
    let instance = addressed_uri(parts);
    let services = state
        .resolve_tenant(&tenant_ref, creation_policy, &instance)
        .await
        .map_err(TenantStateRejection::Resolution)?;
    let target = match snapshot.as_ref() {
        Some(snapshot_id) => services
            .snapshot_target_resolver()
            .resolve(snapshot_id)
            .await
            .map_err(|error| TenantStateRejection::Resolution(error.into_problem(instance)))?,
        None => services.live_target().clone(),
    };
    Ok((services, target, snapshot))
}

impl<S, T> FromRequestParts<S> for ExistingTargetedTenantState<T>
where
    S: TenantStateResolver<T>,
    T: ContextTargetProvider,
{
    type Rejection = TenantStateRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let (services, target, snapshot) = resolve(parts, state, TenantCreationPolicy::MustExist).await?;
        Ok(ExistingTargetedTenantState { services, target, snapshot })
    }
}

impl<S, T> FromRequestParts<S> for ProvisionableTargetedTenantState<T>
where
    S: TenantStateResolver<T>,
    T: ContextTargetProvider,
{
    type Rejection = TenantStateRejection;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let (services, target, snapshot) = resolve(parts, state, TenantCreationPolicy::CreateOnDemand).await?;
        Ok(ProvisionableTargetedTenantState { services, target, snapshot })
    }
}
