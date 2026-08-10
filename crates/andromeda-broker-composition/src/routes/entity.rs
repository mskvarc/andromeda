//! Composition adapters that supply focused broker capabilities to entity HTTP routes.

use crate::{tenant_factory::BrokerTenantFactory, tenant_state::AndromedaState};
use andromeda_http_entity::{
    snapshot_scope::{EntitySnapshotError, EntitySnapshotScope, SnapshotAttributeInfo, SnapshotTypeInfo},
    state::{EntityRouteServices, EntityStateResolver},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use andromeda_postgresql_context_store::tenant_registry::PostgresTenantRegistry;
use andromeda_snapshot::{error::SnapshotError, service::SnapshotService};
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, error::TenancyError, runtime::TenantRuntimeManager, tenant_ref::TenantRef};
use futures::future::BoxFuture;
use serde_json::Value;
use std::sync::Arc;

/// Resolves a complete broker tenant and projects only entity-route capabilities from it.
pub struct BrokerEntityStateResolver {
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
}

impl BrokerEntityStateResolver {
    /// Construct the focused resolver over the process tenant runtime manager.
    pub const fn new(tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> BrokerEntityStateResolver {
        BrokerEntityStateResolver { tenants }
    }
}

impl EntityStateResolver for BrokerEntityStateResolver {
    fn resolve<'a>(&'a self, tenant: &'a TenantRef, creation_policy: TenantCreationPolicy) -> BoxFuture<'a, Result<Arc<EntityRouteServices>, TenancyError>> {
        Box::pin(async move {
            let state: Arc<AndromedaState> = self.tenants.resolve_state(tenant, creation_policy).await?;
            let snapshots: Arc<dyn EntitySnapshotScope> = Arc::new(BrokerEntitySnapshotScope::new(Arc::clone(&state.snapshots)));
            Ok(Arc::new(
                EntityRouteServices::builder()
                    .entities(Arc::clone(&state.entities))
                    .entity_query(Arc::clone(&state.entity_query))
                    .entity_discovery(Arc::clone(&state.entity_discovery))
                    .forwarding(Arc::clone(&state.forwarding))
                    .entity_maps(Arc::clone(&state.entity_maps))
                    .snapshots(snapshots)
                    .build(),
            ))
        })
    }
}

/// Adapts the still-binary snapshot service to the entity binding's frozen-read capability.
struct BrokerEntitySnapshotScope {
    snapshots: Arc<SnapshotService>,
}

impl BrokerEntitySnapshotScope {
    const fn new(snapshots: Arc<SnapshotService>) -> BrokerEntitySnapshotScope {
        BrokerEntitySnapshotScope { snapshots }
    }
}

fn classify_snapshot_error(error: SnapshotError) -> EntitySnapshotError {
    match error {
        SnapshotError::NotFound { .. } => EntitySnapshotError::NotFound { detail: error.to_string() },
        SnapshotError::Validation { .. }
        | SnapshotError::AlreadyExists { .. }
        | SnapshotError::Entity(_)
        | SnapshotError::Temporal(_)
        | SnapshotError::Federation(_)
        | SnapshotError::Request(_)
        | SnapshotError::Representation(_)
        | SnapshotError::Persistence { .. }
        | SnapshotError::Internal { .. } => EntitySnapshotError::Internal { detail: error.to_string() },
    }
}

impl EntitySnapshotScope for BrokerEntitySnapshotScope {
    fn validate<'a>(&'a self, snapshot_id: &'a Uri) -> BoxFuture<'a, Result<(), EntitySnapshotError>> {
        Box::pin(async move {
            self.snapshots
                .validate_for_scoping(snapshot_id)
                .await
                .map(|_| ())
                .map_err(classify_snapshot_error)
        })
    }

    fn entity<'a>(&'a self, snapshot_id: &'a Uri, entity_id: &'a Uri) -> BoxFuture<'a, Result<Option<Value>, EntitySnapshotError>> {
        Box::pin(async move {
            self.snapshots
                .get_snapshot_entity(snapshot_id, entity_id)
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
    ) -> BoxFuture<'a, Result<(Vec<Value>, i64), EntitySnapshotError>> {
        Box::pin(async move {
            self.snapshots
                .query_snapshot_entities(snapshot_id, entity_type, entity_ids, limit, offset)
                .await
                .map_err(classify_snapshot_error)
        })
    }

    fn types<'a>(&'a self, snapshot_id: &'a Uri) -> BoxFuture<'a, Result<Vec<EntityTypeName>, EntitySnapshotError>> {
        Box::pin(async move { self.snapshots.snapshot_types(snapshot_id).await.map_err(classify_snapshot_error) })
    }

    fn type_info<'a>(&'a self, snapshot_id: &'a Uri, type_name: &'a EntityTypeName) -> BoxFuture<'a, Result<Option<SnapshotTypeInfo>, EntitySnapshotError>> {
        Box::pin(async move { self.snapshots.snapshot_type_info(snapshot_id, type_name).await.map_err(classify_snapshot_error) })
    }

    fn attributes<'a>(&'a self, snapshot_id: &'a Uri) -> BoxFuture<'a, Result<Vec<AttributeName>, EntitySnapshotError>> {
        Box::pin(async move { self.snapshots.snapshot_attributes(snapshot_id).await.map_err(classify_snapshot_error) })
    }

    fn attribute_info<'a>(
        &'a self,
        snapshot_id: &'a Uri,
        attribute_name: &'a AttributeName,
    ) -> BoxFuture<'a, Result<Option<SnapshotAttributeInfo>, EntitySnapshotError>> {
        Box::pin(async move {
            self.snapshots
                .snapshot_attribute_info(snapshot_id, attribute_name)
                .await
                .map_err(classify_snapshot_error)
        })
    }
}
