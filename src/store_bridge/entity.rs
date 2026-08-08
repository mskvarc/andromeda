//! `EntityStore` trait implementation for `EntityRepository`.
//!
//! The trait lives in the binary's service layer, so the impl block stays here.
//! Each method delegates to the identically-named inherent method on
//! `EntityRepository` (which now lives in the context-store crate and returns
//! `ContextStoreError`) and maps the error via the `From` impl.

use crate::{core::error::AndromedaError, service::entity::store::EntityStore};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use andromeda_postgresql_context_store::entity::{
    attribute_summary::AttributeSummary,
    query_outcome::QueryOutcome,
    repo::EntityRepository,
    snapshot::EntitySnapshot,
    storage_profile::StorageProfile,
};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

#[async_trait]
impl EntityStore for EntityRepository {
    fn profile(&self) -> &StorageProfile {
        static PROFILE: StorageProfile = StorageProfile::POSTGIS_LTREE;
        &PROFILE
    }

    async fn get(&self, id: &Uri) -> Result<Option<Value>, AndromedaError> {
        Ok(EntityRepository::get(self, id).await?)
    }

    async fn get_batch(&self, ids: &[Uri]) -> Result<HashMap<Uri, Value>, AndromedaError> {
        Ok(EntityRepository::get_batch(self, ids).await?)
    }

    async fn get_snapshot(&self, id: &Uri) -> Result<Option<EntitySnapshot>, AndromedaError> {
        Ok(EntityRepository::get_snapshot(self, id).await?)
    }

    async fn get_snapshot_batch(&self, ids: &[Uri]) -> Result<HashMap<Uri, EntitySnapshot>, AndromedaError> {
        Ok(EntityRepository::get_snapshot_batch(self, ids).await?)
    }

    async fn entities_with_expired_attributes(&self) -> Result<Vec<(Uri, EntitySnapshot)>, AndromedaError> {
        Ok(EntityRepository::entities_with_expired_attributes(self).await?)
    }

    async fn get_sys_attrs(&self, id: &Uri) -> Result<Option<SysAttrsSnapshot>, AndromedaError> {
        Ok(EntityRepository::get_sys_attrs(self, id).await?)
    }

    async fn get_sys_attrs_batch(&self, ids: &[Uri]) -> Result<HashMap<Uri, SysAttrsSnapshot>, AndromedaError> {
        Ok(EntityRepository::get_sys_attrs_batch(self, ids).await?)
    }

    async fn types(&self) -> Result<Vec<EntityTypeName>, AndromedaError> {
        Ok(EntityRepository::types(self).await?)
    }

    async fn get_entity_type_summaries(&self, after: Option<&EntityTypeName>, limit: u32) -> Result<Vec<(EntityTypeName, Vec<AttributeName>)>, AndromedaError> {
        Ok(EntityRepository::get_entity_type_summaries(self, after, limit).await?)
    }

    async fn get_attribute_summary(&self, after: Option<&AttributeName>, limit: u32) -> Result<Vec<AttributeSummary>, AndromedaError> {
        Ok(EntityRepository::get_attribute_summary(self, after, limit).await?)
    }

    async fn get_attribute_info(&self, attr_iri: &AttributeName) -> Result<Option<AttributeSummary>, AndromedaError> {
        Ok(EntityRepository::get_attribute_info(self, attr_iri).await?)
    }

    async fn get_entities_by_type(&self, entity_type: &EntityTypeName, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, AndromedaError> {
        Ok(EntityRepository::get_entities_by_type(self, entity_type, limit, offset).await?)
    }

    async fn get_all_entities(&self, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, AndromedaError> {
        Ok(EntityRepository::get_all_entities(self, limit, offset).await?)
    }

    async fn count_by_type(&self, entity_type: &EntityTypeName) -> Result<i64, AndromedaError> {
        Ok(EntityRepository::count_by_type(self, entity_type).await?)
    }

    async fn query_compiled(&self, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Value>, AndromedaError> {
        Ok(EntityRepository::query_compiled(self, compiled).await?)
    }

    async fn query_ids(&self, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Uri>, AndromedaError> {
        Ok(EntityRepository::query_ids(self, compiled).await?)
    }

    async fn expire_due_entities(&self) -> Result<Vec<Uri>, AndromedaError> {
        Ok(EntityRepository::expire_due_entities(self).await?)
    }
}
