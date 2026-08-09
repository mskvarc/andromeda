use crate::entity::{port::persistence, repo::EntityRepository};
use andromeda_entity::{entity_snapshot::EntitySnapshot, error::EntityError, port::reader::EntityReader};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

#[async_trait]
impl EntityReader for EntityRepository {
    async fn get(&self, id: &Uri) -> Result<Option<Value>, EntityError> {
        EntityRepository::get(self, id).await.map_err(persistence)
    }

    async fn get_batch(&self, ids: &[Uri]) -> Result<HashMap<Uri, Value>, EntityError> {
        EntityRepository::get_batch(self, ids).await.map_err(persistence)
    }

    async fn get_snapshot(&self, id: &Uri) -> Result<Option<EntitySnapshot>, EntityError> {
        EntityRepository::get_snapshot(self, id).await.map_err(persistence)
    }

    async fn get_snapshot_batch(&self, ids: &[Uri]) -> Result<HashMap<Uri, EntitySnapshot>, EntityError> {
        EntityRepository::get_snapshot_batch(self, ids).await.map_err(persistence)
    }

    async fn get_sys_attrs(&self, id: &Uri) -> Result<Option<SysAttrsSnapshot>, EntityError> {
        EntityRepository::get_sys_attrs(self, id).await.map_err(persistence)
    }

    async fn get_sys_attrs_batch(&self, ids: &[Uri]) -> Result<HashMap<Uri, SysAttrsSnapshot>, EntityError> {
        EntityRepository::get_sys_attrs_batch(self, ids).await.map_err(persistence)
    }
}
