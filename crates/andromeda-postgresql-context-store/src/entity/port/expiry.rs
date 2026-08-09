use crate::entity::{port::persistence, repo::EntityRepository};
use andromeda_entity::{entity_snapshot::EntitySnapshot, error::EntityError, expiry::EntityExpiryBatchSize, port::expiry::EntityExpirySource};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
impl EntityExpirySource for EntityRepository {
    async fn entities_due_for_expiry(&self, cutoff: DateTime<Utc>, batch_size: EntityExpiryBatchSize) -> Result<Vec<Uri>, EntityError> {
        EntityRepository::entities_due_for_expiry(self, cutoff, batch_size).await.map_err(persistence)
    }

    async fn entities_with_expired_attributes(
        &self,
        cutoff: DateTime<Utc>,
        batch_size: EntityExpiryBatchSize,
    ) -> Result<Vec<(Uri, EntitySnapshot)>, EntityError> {
        EntityRepository::entities_with_expired_attributes(self, cutoff, batch_size)
            .await
            .map_err(persistence)
    }
}
