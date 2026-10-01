use crate::entity::{port::classify, repo::EntityRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    deletion_outline::{DeletionOutline, DeletionWatchers},
    error::EntityError,
    port::deletion_outline::DeletionOutlineReader,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use async_trait::async_trait;

#[async_trait]
impl DeletionOutlineReader for EntityRepository {
    async fn deletion_outline(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<DeletionOutline>, EntityError> {
        EntityRepository::deletion_outline(self, target, id).await.map_err(|error| classify(&error))
    }

    async fn deletion_watchers(&self, target: &ContextTarget) -> Result<DeletionWatchers, EntityError> {
        EntityRepository::deletion_watchers(self, target).await.map_err(|error| classify(&error))
    }
}
