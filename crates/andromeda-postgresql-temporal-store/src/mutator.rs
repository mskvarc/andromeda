//! [`TemporalMutator`] implementation for [`TemporalRepository`].
//!
//! Thin delegation layer: every method forwards to an existing
//! `TemporalRepository` method, mapping errors at the boundary.

use crate::{entry::TeaEntry, error::TemporalStoreError, params::DeleteScope, repo::TemporalRepository, store::TemporalMutator};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, instance_id::InstanceId, uri::Uri};
use async_trait::async_trait;

#[async_trait]
impl TemporalMutator for TemporalRepository {
    async fn entity_exists(&self, entity_id: &Uri) -> Result<bool, TemporalStoreError> {
        self.tea_exists_for_entity(entity_id).await
    }

    async fn attribute_entries(&self, entity_id: &Uri, attribute_name: &AttributeName) -> Result<Vec<TeaEntry>, TemporalStoreError> {
        self.get_teas_for_entity_attr(entity_id, attribute_name).await
    }

    async fn attribute_names_for_entity(&self, entity_id: &Uri) -> Result<Vec<AttributeName>, TemporalStoreError> {
        self.get_attribute_names_for_entity(entity_id).await
    }

    async fn delete_all_for_entity(&self, entity_id: &Uri) -> Result<u64, TemporalStoreError> {
        self.delete_teas_for_entity(entity_id).await
    }

    async fn delete_attribute(
        &self,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        dataset_id: Option<&Uri>,
        scope: DeleteScope,
    ) -> Result<u64, TemporalStoreError> {
        self.delete_tea(entity_id, attribute_name, dataset_id, scope).await
    }

    async fn get_instance(&self, instance_id: &InstanceId, entity_id: &Uri, attribute_name: &AttributeName) -> Result<bool, TemporalStoreError> {
        let row = TemporalRepository::get_instance(self, instance_id, entity_id, attribute_name).await?;
        Ok(row.is_some())
    }

    async fn delete_instance(&self, instance_id: &InstanceId, entity_id: &Uri, attribute_name: &AttributeName) -> Result<(), TemporalStoreError> {
        TemporalRepository::delete_instance(self, instance_id, entity_id, attribute_name).await
    }
}
