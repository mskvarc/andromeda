//! [`TemporalMutator`] implementation for [`TemporalRepository`].
//!
//! Thin delegation layer: every method forwards to an existing
//! `TemporalRepository` method, mapping `AndromedaError` to
//! `TemporalStoreError` at the boundary.

use crate::{
    repository::temporal::{params::DeleteScope, repo::TemporalRepository},
    service::temporal::store::{TemporalMutator, TemporalStoreError},
};
use async_trait::async_trait;

#[async_trait]
impl TemporalMutator for TemporalRepository {
    async fn entity_exists(&self, entity_id: &str) -> Result<bool, TemporalStoreError> {
        self.tea_exists_for_entity(entity_id)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))
    }

    async fn attribute_entries(&self, entity_id: &str, attribute_name: &str) -> Result<Vec<crate::repository::temporal::entry::TeaEntry>, TemporalStoreError> {
        self.get_teas_for_entity_attr(entity_id, attribute_name)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))
    }

    async fn attribute_names_for_entity(&self, entity_id: &str) -> Result<Vec<String>, TemporalStoreError> {
        self.get_attribute_names_for_entity(entity_id)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))
    }

    async fn delete_all_for_entity(&self, entity_id: &str) -> Result<u64, TemporalStoreError> {
        self.delete_teas_for_entity(entity_id)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))
    }

    async fn delete_attribute(&self, entity_id: &str, attribute_name: &str, dataset_id: Option<&str>, scope: DeleteScope) -> Result<u64, TemporalStoreError> {
        self.delete_tea(entity_id, attribute_name, dataset_id, scope)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))
    }

    async fn get_instance(&self, instance_id: &str, entity_id: &str, attribute_name: &str) -> Result<bool, TemporalStoreError> {
        let row = TemporalRepository::get_instance(self, instance_id, entity_id, attribute_name)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))?;
        Ok(row.is_some())
    }

    async fn delete_instance(&self, instance_id: &str, entity_id: &str, attribute_name: &str) -> Result<(), TemporalStoreError> {
        TemporalRepository::delete_instance(self, instance_id, entity_id, attribute_name)
            .await
            .map_err(|e| TemporalStoreError::Query(e.to_string()))
    }
}
