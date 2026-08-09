use crate::entity::{port::persistence, repo::EntityRepository};
use andromeda_entity::{entity_snapshot::EntitySnapshot, error::EntityError, port::expiry::EntityExpirySource};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use async_trait::async_trait;

#[async_trait]
impl EntityExpirySource for EntityRepository {
    async fn entities_due_for_expiry(&self) -> Result<Vec<Uri>, EntityError> {
        EntityRepository::entities_due_for_expiry(self).await.map_err(persistence)
    }

    async fn entities_with_expired_attributes(&self) -> Result<Vec<(Uri, EntitySnapshot)>, EntityError> {
        EntityRepository::entities_with_expired_attributes(self).await.map_err(persistence)
    }
}
