//! [`TemporalMutator`] implementation for [`TemporalRepository`].
//!
//! Thin delegation layer: every method forwards to an existing
//! `TemporalRepository` method, mapping errors at the boundary. Every question
//! it answers is answered from the temporal catalogue, never from current
//! state, because clause 5.6.6 leaves the Temporal Evolution standing when the
//! entity itself is deleted.

use crate::{repo::TemporalRepository, temporal_catalog_read::AttributeStreamScope};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, instance_identity::InstanceIdentity, uri::Uri};
use andromeda_temporal::{
    error::TemporalError,
    port::mutator::{TemporalDeleteScope, TemporalMutator},
};
use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
impl TemporalMutator for TemporalRepository {
    async fn entity_exists(&self, entity_id: &Uri) -> Result<bool, TemporalError> {
        self.temporal_entity_exists(entity_id).await.map_err(TemporalError::from)
    }

    async fn attribute_exists(&self, entity_id: &Uri, attribute_name: &AttributeName) -> Result<bool, TemporalError> {
        self.attribute_stream_exists(entity_id, attribute_name).await.map_err(TemporalError::from)
    }

    async fn attribute_names_for_entity(&self, entity_id: &Uri) -> Result<Vec<AttributeName>, TemporalError> {
        self.temporal_attribute_names(entity_id).await.map_err(TemporalError::from)
    }

    async fn delete_all_for_entity(&self, entity_id: &Uri) -> Result<u64, TemporalError> {
        self.delete_temporal_evolution(entity_id)
            .await
            .map(|deletion| deletion.streams)
            .map_err(TemporalError::from)
    }

    async fn delete_attribute(
        &self,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        dataset_id: Option<&Uri>,
        scope: TemporalDeleteScope,
    ) -> Result<u64, TemporalError> {
        let scope = match scope {
            TemporalDeleteScope::All => AttributeStreamScope::All,
            TemporalDeleteScope::Single => AttributeStreamScope::Single,
        };
        self.delete_attribute_streams(entity_id, attribute_name, dataset_id, scope)
            .await
            .map_err(TemporalError::from)
    }

    async fn instance_exists(&self, instance: &InstanceIdentity, entity_id: &Uri, attribute_name: &AttributeName) -> Result<bool, TemporalError> {
        let row = TemporalRepository::get_instance(self, instance, entity_id, attribute_name).await?;
        Ok(row.is_some())
    }

    async fn update_instance(
        &self,
        instance: &InstanceIdentity,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        expanded_instance: &Value,
    ) -> Result<(), TemporalError> {
        self.update_attribute_instance(entity_id, attribute_name, instance, expanded_instance)
            .await
            .map_err(TemporalError::from)
    }

    async fn delete_instance(&self, instance: &InstanceIdentity, entity_id: &Uri, attribute_name: &AttributeName) -> Result<(), TemporalError> {
        TemporalRepository::delete_instance(self, instance, entity_id, attribute_name)
            .await
            .map_err(TemporalError::from)
    }
}
