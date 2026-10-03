use crate::entity::{port::classify, repo::EntityRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    error::EntityError,
    port::{query::EntityQueryStore, retrieved_entity::RetrievedEntity},
    query_outcome::QueryOutcome,
    storage_profile::StorageProfile,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, entity::compiled::CompiledEntityQuery, page_window::PageWindow};
use async_trait::async_trait;

#[async_trait]
impl EntityQueryStore for EntityRepository {
    fn profile(&self) -> &StorageProfile {
        &EntityRepository::PROFILE
    }

    async fn query_compiled(
        &self,
        target: &ContextTarget,
        compiled: &CompiledEntityQuery,
        window: &PageWindow,
        count: CountPolicy,
    ) -> Result<QueryOutcome<RetrievedEntity>, EntityError> {
        EntityRepository::query_compiled(self, target, compiled, window, count)
            .await
            .map_err(|error| classify(&error))
    }

    async fn query_ids(&self, target: &ContextTarget, compiled: &CompiledEntityQuery, window: &PageWindow) -> Result<QueryOutcome<Identifier>, EntityError> {
        EntityRepository::query_ids(self, target, compiled, window)
            .await
            .map_err(|error| classify(&error))
    }
}
