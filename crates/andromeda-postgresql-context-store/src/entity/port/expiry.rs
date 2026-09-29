use crate::entity::{port::classify, repo::EntityRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{entity_snapshot::EntitySnapshot, error::EntityError, expiry::EntityExpiryBatchSize, port::expiry::EntityExpirySource};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
impl EntityExpirySource for EntityRepository {
    async fn entities_due_for_expiry(
        &self,
        target: &ContextTarget,
        cutoff: DateTime<Utc>,
        batch_size: EntityExpiryBatchSize,
    ) -> Result<Vec<Identifier>, EntityError> {
        EntityRepository::entities_due_for_expiry(self, target, cutoff, batch_size)
            .await
            .map_err(|error| classify(&error))
    }

    async fn entities_with_expired_attributes(
        &self,
        target: &ContextTarget,
        cutoff: DateTime<Utc>,
        batch_size: EntityExpiryBatchSize,
    ) -> Result<Vec<(Identifier, EntitySnapshot)>, EntityError> {
        EntityRepository::entities_with_expired_attributes(self, target, cutoff, batch_size)
            .await
            .map_err(|error| classify(&error))
    }
}
