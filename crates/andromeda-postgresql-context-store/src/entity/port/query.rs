use crate::entity::{port::persistence, repo::EntityRepository};
use andromeda_entity::{error::EntityError, port::query::EntityQueryStore, query_outcome::QueryOutcome, storage_profile::StorageProfile};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
impl EntityQueryStore for EntityRepository {
    fn profile(&self) -> &StorageProfile {
        &EntityRepository::PROFILE
    }

    async fn query_compiled(&self, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Value>, EntityError> {
        EntityRepository::query_compiled(self, compiled).await.map_err(persistence)
    }

    async fn query_ids(&self, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Uri>, EntityError> {
        EntityRepository::query_ids(self, compiled).await.map_err(persistence)
    }
}
