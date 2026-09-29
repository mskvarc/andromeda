use crate::entity::{port::classify, repo::EntityRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{error::EntityError, port::query::EntityQueryStore, query_outcome::QueryOutcome, storage_profile::StorageProfile};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
impl EntityQueryStore for EntityRepository {
    fn profile(&self) -> &StorageProfile {
        &EntityRepository::PROFILE
    }

    async fn query_compiled(&self, target: &ContextTarget, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Value>, EntityError> {
        EntityRepository::query_compiled(self, target, compiled).await.map_err(|error| classify(&error))
    }

    async fn query_ids(&self, target: &ContextTarget, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Identifier>, EntityError> {
        EntityRepository::query_ids(self, target, compiled).await.map_err(|error| classify(&error))
    }
}
