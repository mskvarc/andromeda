use crate::entity::{port::classify, repo::EntityRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    port::{reader::EntityReader, retrieved_entity::RetrievedEntity},
};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::projection::returned_attributes::ReturnedAttributes;
use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};

#[async_trait]
impl EntityReader for EntityRepository {
    async fn get(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
        EntityRepository::get(self, target, id)
            .await
            .map(|state| state.map(Arc::new))
            .map_err(|error| classify(&error))
    }

    async fn get_batch(&self, target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
        EntityRepository::get_batch(self, target, ids).await.map_err(|error| classify(&error))
    }

    async fn get_with_sys_attrs(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        returned: &ReturnedAttributes<'_>,
    ) -> Result<Option<RetrievedEntity>, EntityError> {
        EntityRepository::get_with_sys_attrs(self, target, id, returned)
            .await
            .map_err(|error| classify(&error))
    }

    async fn get_snapshot(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        EntityRepository::get_snapshot(self, target, id).await.map_err(|error| classify(&error))
    }

    async fn get_snapshot_batch(&self, target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        EntityRepository::get_snapshot_batch(self, target, ids).await.map_err(|error| classify(&error))
    }

    async fn get_sys_attrs(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<SysAttrsSnapshot>, EntityError> {
        EntityRepository::get_sys_attrs(self, target, id).await.map_err(|error| classify(&error))
    }

    async fn get_sys_attrs_batch(&self, target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        EntityRepository::get_sys_attrs_batch(self, target, ids).await.map_err(|error| classify(&error))
    }
}
