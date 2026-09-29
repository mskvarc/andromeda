use crate::entity::{port::classify, repo::EntityRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{entity_snapshot::EntitySnapshot, error::EntityError, port::reader::EntityReader};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

#[async_trait]
impl EntityReader for EntityRepository {
    async fn get(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<Value>, EntityError> {
        EntityRepository::get(self, target, id).await.map_err(|error| classify(&error))
    }

    async fn get_batch(&self, target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, Value>, EntityError> {
        EntityRepository::get_batch(self, target, ids).await.map_err(|error| classify(&error))
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
