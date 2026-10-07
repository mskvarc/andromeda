//! Every question the mutator answers is answered from the temporal catalogue, never from current state, because clause 5.6.6 leaves the Temporal Evolution standing when the entity itself is deleted.

use crate::{attribute_stream_deletion::AttributeStreamScope, repo::TemporalRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, instance_identity::InstanceIdentity};
use andromeda_ngsi_ld_query::temporal::history_delete::TemporalHistoryDeleteScope;
use andromeda_temporal::{
    error::TemporalError,
    observed_instance::ObservedInstance,
    port::mutator::{TemporalDeleteScope, TemporalMutator},
};
use async_trait::async_trait;

#[async_trait]
impl TemporalMutator for TemporalRepository {
    async fn entity_exists(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<bool, TemporalError> {
        self.temporal_entity_exists(target, entity_id).await.map_err(TemporalError::from)
    }

    async fn attribute_exists(&self, target: &ContextTarget, entity_id: &Identifier, attribute_name: &AttributeName) -> Result<bool, TemporalError> {
        self.attribute_stream_exists(target, entity_id, attribute_name)
            .await
            .map_err(TemporalError::from)
    }

    async fn attribute_names_for_entity(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<Vec<AttributeName>, TemporalError> {
        self.temporal_attribute_names(target, entity_id).await.map_err(TemporalError::from)
    }

    async fn delete_all_for_entity(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<u64, TemporalError> {
        self.delete_temporal_evolution(target, entity_id)
            .await
            .map(|deletion| deletion.streams)
            .map_err(TemporalError::from)
    }

    async fn delete_attribute(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
        dataset_id: Option<&Identifier>,
        scope: TemporalDeleteScope,
    ) -> Result<u64, TemporalError> {
        let scope = match scope {
            TemporalDeleteScope::All => AttributeStreamScope::All,
            TemporalDeleteScope::Single => AttributeStreamScope::Single,
        };
        self.delete_attribute_streams(target, entity_id, attribute_name, dataset_id, scope)
            .await
            .map_err(TemporalError::from)
    }

    async fn instance_exists(
        &self,
        target: &ContextTarget,
        instance: &InstanceIdentity,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
    ) -> Result<bool, TemporalError> {
        let row = TemporalRepository::get_instance(self, target, instance, entity_id, attribute_name).await?;
        Ok(row.is_some())
    }

    async fn update_instance(
        &self,
        target: &ContextTarget,
        instance: &InstanceIdentity,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
        replacement: &ObservedInstance,
    ) -> Result<(), TemporalError> {
        self.update_attribute_instance(target, entity_id, attribute_name, instance, replacement)
            .await
            .map_err(TemporalError::from)
    }

    async fn delete_instance(
        &self,
        target: &ContextTarget,
        instance: &InstanceIdentity,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
    ) -> Result<(), TemporalError> {
        TemporalRepository::delete_instance(self, target, instance, entity_id, attribute_name)
            .await
            .map_err(TemporalError::from)
    }

    async fn count_history_instances(
        &self,
        target: &ContextTarget,
        entity_ids: &[Identifier],
        scope: &TemporalHistoryDeleteScope,
    ) -> Result<u64, TemporalError> {
        self.count_windowed_history(target, entity_ids, scope).await.map_err(TemporalError::from)
    }

    async fn delete_history_instances(
        &self,
        target: &ContextTarget,
        entity_ids: &[Identifier],
        scope: &TemporalHistoryDeleteScope,
    ) -> Result<u64, TemporalError> {
        self.delete_windowed_history(target, entity_ids, scope).await.map_err(TemporalError::from)
    }
}
