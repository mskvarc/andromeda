use crate::entity::{port::classify, repo::EntityRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    attribute_summary::AttributeSummary,
    entity_type_summary::EntityTypeSummary,
    error::EntityError,
    port::discovery::{AttributeEntityTypes, EntityDiscoveryStore, EntityTypeAttributes},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use async_trait::async_trait;

#[async_trait]
impl EntityDiscoveryStore for EntityRepository {
    async fn types(&self, target: &ContextTarget) -> Result<Vec<EntityTypeName>, EntityError> {
        EntityRepository::census_types(self, target).await.map_err(|error| classify(&error))
    }

    async fn entity_type_attributes(&self, target: &ContextTarget) -> Result<Vec<EntityTypeAttributes>, EntityError> {
        EntityRepository::census_entity_type_attributes(self, target)
            .await
            .map_err(|error| classify(&error))
    }

    async fn entity_type_summary(&self, target: &ContextTarget, entity_type: &EntityTypeName) -> Result<Option<EntityTypeSummary>, EntityError> {
        EntityRepository::census_entity_type_summary(self, target, entity_type)
            .await
            .map_err(|error| classify(&error))
    }

    async fn attribute_entity_types(&self, target: &ContextTarget) -> Result<Vec<AttributeEntityTypes>, EntityError> {
        EntityRepository::census_attribute_entity_types(self, target)
            .await
            .map_err(|error| classify(&error))
    }

    async fn attribute_summary(&self, target: &ContextTarget, attribute: &AttributeName) -> Result<Option<AttributeSummary>, EntityError> {
        EntityRepository::census_attribute_summary(self, target, attribute)
            .await
            .map_err(|error| classify(&error))
    }
}
