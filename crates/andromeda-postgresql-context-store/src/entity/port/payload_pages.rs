use crate::entity::{port::classify, repo::EntityRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{error::EntityError, port::payload_pages::EntityPayloadPages};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use async_trait::async_trait;

#[async_trait]
impl EntityPayloadPages for EntityRepository {
    async fn get_entities_by_type(
        &self,
        target: &ContextTarget,
        entity_type: &EntityTypeName,
        after: Option<&Identifier>,
        limit: u16,
    ) -> Result<Vec<EntityState>, EntityError> {
        EntityRepository::get_entities_by_type(self, target, entity_type, after, limit)
            .await
            .map_err(|error| classify(&error))
    }

    async fn get_all_entities(&self, target: &ContextTarget, after: Option<&Identifier>, limit: u16) -> Result<Vec<EntityState>, EntityError> {
        EntityRepository::get_all_entities(self, target, after, limit)
            .await
            .map_err(|error| classify(&error))
    }
}
