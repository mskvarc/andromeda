//! Composition adapters that supply focused broker capabilities to temporal HTTP routes.

use crate::{
    api::{state::AndromedaState, tenant_factory::BrokerTenantFactory},
    error::AndromedaError,
    service::snapshot::SnapshotService,
};
use andromeda_http_temporal::{
    snapshot_scope::{TemporalSnapshotError, TemporalSnapshotScope},
    state::{TemporalRouteServices, TemporalStateResolver},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, runtime::TenantRuntimeManager, tenant_ref::TenantRef};
use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;

/// Resolves a complete broker tenant and projects only temporal-route capabilities from it.
pub struct BrokerTemporalStateResolver {
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
}

impl BrokerTemporalStateResolver {
    /// Construct the focused resolver over the process tenant runtime manager.
    pub const fn new(tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> BrokerTemporalStateResolver {
        BrokerTemporalStateResolver { tenants }
    }
}

impl TemporalStateResolver for BrokerTemporalStateResolver {
    fn resolve<'a>(&'a self, tenant: &'a TenantRef, creation_policy: TenantCreationPolicy) -> BoxFuture<'a, Result<Arc<TemporalRouteServices>, TenancyError>> {
        Box::pin(async move {
            let state: Arc<AndromedaState> = self.tenants.resolve_state(tenant, creation_policy).await?;
            let snapshots: Arc<dyn TemporalSnapshotScope> = Arc::new(BrokerTemporalSnapshotScope::new(Arc::clone(&state.snapshots)));
            Ok(Arc::new(
                TemporalRouteServices::builder()
                    .temporal(Arc::clone(&state.temporal))
                    .forwarding(Arc::clone(&state.forwarding))
                    .entity_maps(Arc::clone(&state.entity_maps))
                    .snapshots(snapshots)
                    .build(),
            ))
        })
    }
}

/// Adapts the still-binary snapshot service to the temporal binding's frozen-read capability.
struct BrokerTemporalSnapshotScope {
    snapshots: Arc<SnapshotService>,
}

impl BrokerTemporalSnapshotScope {
    const fn new(snapshots: Arc<SnapshotService>) -> BrokerTemporalSnapshotScope {
        BrokerTemporalSnapshotScope { snapshots }
    }
}

fn classify_snapshot_error(error: AndromedaError) -> TemporalSnapshotError {
    if error.is_snapshot_not_found() {
        TemporalSnapshotError::NotFound { detail: error.to_string() }
    } else {
        TemporalSnapshotError::Internal { detail: error.to_string() }
    }
}

impl TemporalSnapshotScope for BrokerTemporalSnapshotScope {
    fn validate<'a>(&'a self, snapshot_id: &'a Uri) -> BoxFuture<'a, Result<(), TemporalSnapshotError>> {
        Box::pin(async move {
            self.snapshots
                .validate_for_scoping(snapshot_id)
                .await
                .map(|_| ())
                .map_err(classify_snapshot_error)
        })
    }

    fn entity<'a>(&'a self, snapshot_id: &'a Uri, entity_id: &'a Uri) -> BoxFuture<'a, Result<Option<Value>, TemporalSnapshotError>> {
        Box::pin(async move {
            self.snapshots
                .get_snapshot_temporal_entity(snapshot_id, entity_id)
                .await
                .map_err(classify_snapshot_error)
        })
    }

    fn entities<'a>(
        &'a self,
        snapshot_id: &'a Uri,
        entity_type: Option<&'a TypeExprRaw>,
        entity_ids: Option<&'a [Uri]>,
        limit: i64,
        offset: i64,
    ) -> BoxFuture<'a, Result<(Vec<Value>, i64), TemporalSnapshotError>> {
        Box::pin(async move {
            self.snapshots
                .query_snapshot_temporal_entities(snapshot_id, entity_type, entity_ids, limit, offset)
                .await
                .map_err(classify_snapshot_error)
        })
    }
}
