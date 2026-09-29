use crate::entity::{port::classify, repo::EntityRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{error::EntityError, port::payload_pages::EntityPayloadPages};
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
impl EntityPayloadPages for EntityRepository {
    async fn get_entities_by_type(
        &self,
        target: &ContextTarget,
        entity_type: &EntityTypeName,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<Value>, EntityError> {
        EntityRepository::get_entities_by_type(self, target, entity_type, limit, offset)
            .await
            .map_err(|error| classify(&error))
    }

    async fn get_all_entities(&self, target: &ContextTarget, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, EntityError> {
        EntityRepository::get_all_entities(self, target, limit, offset)
            .await
            .map_err(|error| classify(&error))
    }
}
