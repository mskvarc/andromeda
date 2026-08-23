//! Immediate temporal mutation preconditions and operations.

use crate::error::TemporalError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, instance_identity::InstanceIdentity, uri::Uri};
use async_trait::async_trait;
use serde_json::Value;

/// Scope of one temporal attribute deletion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalDeleteScope {
    /// Delete every dataset instance of the attribute.
    All,
    /// Delete only the selected dataset instance, including the default instance.
    Single,
}

/// Immediate mutations and existence checks required by temporal policy.
#[async_trait]
pub trait TemporalMutator: Send + Sync {
    /// Return whether temporal history exists for an entity.
    #[must_use = "temporal entity existence checks must be awaited"]
    async fn entity_exists(&self, target: &ContextTarget, entity_id: &Uri) -> Result<bool, TemporalError>;

    /// Return whether temporal history exists for an entity attribute.
    #[must_use = "temporal attribute existence checks must be awaited"]
    async fn attribute_exists(&self, target: &ContextTarget, entity_id: &Uri, attribute_name: &AttributeName) -> Result<bool, TemporalError>;

    /// Return expanded attribute names known for an entity.
    #[must_use = "temporal attribute-name reads must be awaited"]
    async fn attribute_names_for_entity(&self, target: &ContextTarget, entity_id: &Uri) -> Result<Vec<AttributeName>, TemporalError>;

    /// Delete every temporal observation for an entity and return the affected catalogue rows.
    #[must_use = "temporal entity deletions must be awaited"]
    async fn delete_all_for_entity(&self, target: &ContextTarget, entity_id: &Uri) -> Result<u64, TemporalError>;

    /// Delete one temporal attribute scope and return the affected catalogue rows.
    #[must_use = "temporal attribute deletions must be awaited"]
    async fn delete_attribute(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        dataset_id: Option<&Uri>,
        scope: TemporalDeleteScope,
    ) -> Result<u64, TemporalError>;

    /// Return whether the selected temporal attribute instance exists.
    #[must_use = "temporal instance existence checks must be awaited"]
    async fn instance_exists(
        &self,
        target: &ContextTarget,
        instance: &InstanceIdentity,
        entity_id: &Uri,
        attribute_name: &AttributeName,
    ) -> Result<bool, TemporalError>;

    /// Replace one temporal attribute instance immediately.
    #[must_use = "temporal instance updates must be awaited"]
    async fn update_instance(
        &self,
        target: &ContextTarget,
        instance: &InstanceIdentity,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        expanded_instance: &Value,
    ) -> Result<(), TemporalError>;

    /// Delete one temporal attribute instance immediately.
    #[must_use = "temporal instance deletions must be awaited"]
    async fn delete_instance(
        &self,
        target: &ContextTarget,
        instance: &InstanceIdentity,
        entity_id: &Uri,
        attribute_name: &AttributeName,
    ) -> Result<(), TemporalError>;
}
