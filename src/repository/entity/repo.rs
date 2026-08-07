//! The PostgreSQL entity store.
//!
//! Holds the tenant pool plus the two things a pushed-down `q` predicate needs:
//! the attribute-name dictionary the projection is keyed on, and the barrier
//! that says the projection has caught up. Both are properties of this store,
//! not of the policy above it, which is why they live here and not in the port.

use crate::{
    core::error::AndromedaError,
    repository::{attribute_current::queue::AttributeCurrentBarrier, attribute_names::AttributeNameCache},
    service::entity::{
        query_outcome::QueryOutcome,
        storage_profile::StorageProfile,
        store::{AttributeSummary, EntitySnapshot, EntityStore},
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use async_trait::async_trait;
use serde_json::Value;
use sqlx::postgres::PgPool;
use std::{collections::HashMap, sync::Arc};

/// Repository for persisting and querying NGSI-LD entities.
#[derive(Debug)]
pub struct EntityRepository {
    pub(super) pool: PgPool,
    pub(super) attribute_names: Arc<AttributeNameCache>,
    pub(super) attribute_current: AttributeCurrentBarrier,
}

impl EntityRepository {
    /// What this store can be asked to do with a compiled query.
    ///
    /// A constant rather than a probe: `migrations/tenant/` creates the PostGIS
    /// geometry columns and the `ltree` scope columns, so a pool that migrated
    /// has them and one that could not migrate never becomes a store. That is
    /// the difference between these axes and the TimescaleDB edition, which is
    /// genuinely per-pool runtime state.
    pub const PROFILE: StorageProfile = StorageProfile::POSTGIS_LTREE;

    /// Create a new entity repository backed by the given connection pool.
    pub fn new(pool: &PgPool, attribute_names: Arc<AttributeNameCache>, attribute_current: AttributeCurrentBarrier) -> EntityRepository {
        EntityRepository {
            pool: pool.clone(), // clone: Arc ref-count bump
            attribute_names,
            attribute_current,
        }
    }
}

#[async_trait]
impl EntityStore for EntityRepository {
    fn profile(&self) -> &StorageProfile {
        &EntityRepository::PROFILE
    }

    async fn get(&self, id: &Uri) -> Result<Option<Value>, AndromedaError> {
        EntityRepository::get(self, id).await
    }

    async fn get_batch(&self, ids: &[Uri]) -> Result<HashMap<Uri, Value>, AndromedaError> {
        EntityRepository::get_batch(self, ids).await
    }

    async fn get_snapshot(&self, id: &Uri) -> Result<Option<EntitySnapshot>, AndromedaError> {
        EntityRepository::get_snapshot(self, id).await
    }

    async fn get_snapshot_batch(&self, ids: &[Uri]) -> Result<HashMap<Uri, EntitySnapshot>, AndromedaError> {
        EntityRepository::get_snapshot_batch(self, ids).await
    }

    async fn entities_with_expired_attributes(&self) -> Result<Vec<(Uri, EntitySnapshot)>, AndromedaError> {
        EntityRepository::entities_with_expired_attributes(self).await
    }

    async fn get_sys_attrs(&self, id: &Uri) -> Result<Option<SysAttrsSnapshot>, AndromedaError> {
        EntityRepository::get_sys_attrs(self, id).await
    }

    async fn get_sys_attrs_batch(&self, ids: &[Uri]) -> Result<HashMap<Uri, SysAttrsSnapshot>, AndromedaError> {
        EntityRepository::get_sys_attrs_batch(self, ids).await
    }

    async fn types(&self) -> Result<Vec<EntityTypeName>, AndromedaError> {
        EntityRepository::types(self).await
    }

    async fn get_entity_type_summaries(&self, after: Option<&EntityTypeName>, limit: u32) -> Result<Vec<(EntityTypeName, Vec<AttributeName>)>, AndromedaError> {
        EntityRepository::get_entity_type_summaries(self, after, limit).await
    }

    async fn get_attribute_summary(&self, after: Option<&AttributeName>, limit: u32) -> Result<Vec<AttributeSummary>, AndromedaError> {
        EntityRepository::get_attribute_summary(self, after, limit).await
    }

    async fn get_attribute_info(&self, attr_iri: &AttributeName) -> Result<Option<AttributeSummary>, AndromedaError> {
        EntityRepository::get_attribute_info(self, attr_iri).await
    }

    async fn get_entities_by_type(&self, entity_type: &EntityTypeName, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, AndromedaError> {
        EntityRepository::get_entities_by_type(self, entity_type, limit, offset).await
    }

    async fn get_all_entities(&self, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, AndromedaError> {
        EntityRepository::get_all_entities(self, limit, offset).await
    }

    async fn count_by_type(&self, entity_type: &EntityTypeName) -> Result<i64, AndromedaError> {
        EntityRepository::count_by_type(self, entity_type).await
    }

    async fn query_compiled(&self, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Value>, AndromedaError> {
        EntityRepository::query_compiled(self, compiled).await
    }

    async fn query_ids(&self, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Uri>, AndromedaError> {
        EntityRepository::query_ids(self, compiled).await
    }

    async fn expire_due_entities(&self) -> Result<Vec<Uri>, AndromedaError> {
        EntityRepository::expire_due_entities(self).await
    }
}
