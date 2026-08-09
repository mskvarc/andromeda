//! [`TemporalMutator`] implementation for [`TemporalRepository`].
//!
//! Thin delegation layer: every method forwards to an existing
//! `TemporalRepository` method, mapping errors at the boundary.

use crate::{params::DeleteScope, repo::TemporalRepository};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, instance_id::InstanceId, uri::Uri};
use andromeda_temporal::{
    error::TemporalError,
    port::mutator::{TemporalDeleteScope, TemporalMutator},
};
use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
impl TemporalMutator for TemporalRepository {
    async fn entity_exists(&self, entity_id: &Uri) -> Result<bool, TemporalError> {
        self.tea_exists_for_entity(entity_id).await.map_err(TemporalError::from)
    }

    async fn attribute_exists(&self, entity_id: &Uri, attribute_name: &AttributeName) -> Result<bool, TemporalError> {
        self.get_teas_for_entity_attr(entity_id, attribute_name)
            .await
            .map(|entries| !entries.is_empty())
            .map_err(TemporalError::from)
    }

    async fn attribute_names_for_entity(&self, entity_id: &Uri) -> Result<Vec<AttributeName>, TemporalError> {
        self.get_attribute_names_for_entity(entity_id).await.map_err(TemporalError::from)
    }

    async fn delete_all_for_entity(&self, entity_id: &Uri) -> Result<u64, TemporalError> {
        self.delete_teas_for_entity(entity_id).await.map_err(TemporalError::from)
    }

    async fn delete_attribute(
        &self,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        dataset_id: Option<&Uri>,
        scope: TemporalDeleteScope,
    ) -> Result<u64, TemporalError> {
        let scope = match scope {
            TemporalDeleteScope::All => DeleteScope::All,
            TemporalDeleteScope::Single => DeleteScope::Single,
        };
        self.delete_tea(entity_id, attribute_name, dataset_id, scope).await.map_err(TemporalError::from)
    }

    async fn instance_exists(&self, instance_id: &InstanceId, entity_id: &Uri, attribute_name: &AttributeName) -> Result<bool, TemporalError> {
        let row = TemporalRepository::get_instance(self, instance_id, entity_id, attribute_name).await?;
        Ok(row.is_some())
    }

    async fn update_instance(
        &self,
        instance_id: &InstanceId,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        expanded_instance: &Value,
    ) -> Result<(), TemporalError> {
        self.update_attribute_instance(entity_id, attribute_name, instance_id, expanded_instance)
            .await
            .map_err(TemporalError::from)
    }

    async fn delete_instance(&self, instance_id: &InstanceId, entity_id: &Uri, attribute_name: &AttributeName) -> Result<(), TemporalError> {
        TemporalRepository::delete_instance(self, instance_id, entity_id, attribute_name)
            .await
            .map_err(TemporalError::from)
    }
}
