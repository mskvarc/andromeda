use crate::entity::{port::persistence, repo::EntityRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    attribute_summary::AttributeSummary,
    error::EntityError,
    port::discovery::{EntityDiscoveryStore, EntityTypeAttributes},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
impl EntityDiscoveryStore for EntityRepository {
    async fn types(&self, target: &ContextTarget) -> Result<Vec<EntityTypeName>, EntityError> {
        EntityRepository::types(self, target).await.map_err(|error| persistence(&error))
    }

    async fn get_entity_type_summaries(
        &self,
        target: &ContextTarget,
        after: Option<&EntityTypeName>,
        limit: u32,
    ) -> Result<Vec<EntityTypeAttributes>, EntityError> {
        EntityRepository::get_entity_type_summaries(self, target, after, limit)
            .await
            .map_err(|error| persistence(&error))
    }

    async fn get_attribute_summary(&self, target: &ContextTarget, after: Option<&AttributeName>, limit: u32) -> Result<Vec<AttributeSummary>, EntityError> {
        EntityRepository::get_attribute_summary(self, target, after, limit)
            .await
            .map_err(|error| persistence(&error))
    }

    async fn get_attribute_info(&self, target: &ContextTarget, attribute: &AttributeName) -> Result<Option<AttributeSummary>, EntityError> {
        EntityRepository::get_attribute_info(self, target, attribute)
            .await
            .map_err(|error| persistence(&error))
    }

    async fn get_entities_by_type(
        &self,
        target: &ContextTarget,
        entity_type: &EntityTypeName,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> Result<Vec<Value>, EntityError> {
        EntityRepository::get_entities_by_type(self, target, entity_type, limit, offset)
            .await
            .map_err(|error| persistence(&error))
    }

    async fn get_all_entities(&self, target: &ContextTarget, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, EntityError> {
        EntityRepository::get_all_entities(self, target, limit, offset)
            .await
            .map_err(|error| persistence(&error))
    }

    async fn count_by_type(&self, target: &ContextTarget, entity_type: &EntityTypeName) -> Result<i64, EntityError> {
        EntityRepository::count_by_type(self, target, entity_type)
            .await
            .map_err(|error| persistence(&error))
    }
}
