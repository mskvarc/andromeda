use crate::entity::{port::classify, repo::EntityRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{error::EntityError, port::entity_types::EntityTypeReader};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use async_trait::async_trait;

#[async_trait]
impl EntityTypeReader for EntityRepository {
    async fn entity_types(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<Vec<EntityTypeName>>, EntityError> {
        EntityRepository::entity_types(self, target, id).await.map_err(|error| classify(&error))
    }
}
