//! The [`EntityQueryService`] the read-path unit tests are built on.
//!
//! Several read-path modules exercise service methods that touch only the
//! JSON-LD engine — term expansion, projection compilation, nested-projection
//! derivation — and never read a row. They all need the same service wired to
//! a store that is never called, so that store lives here once instead of
//! being restated in each test module.

use crate::{
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    port::{query::EntityQueryStore, reader::EntityReader},
    query::EntityQueryService,
    query_outcome::QueryOutcome,
    spatial_acceleration::SpatialAcceleration,
    storage_profile::StorageProfile,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery;
use andromeda_tenancy::tenant_ref::TenantRef;
use async_trait::async_trait;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

/// A store whose every method is an error: the tests built on it never read.
struct UnusedStore;

/// The error a call into [`UnusedStore`] returns, naming the method so a test
/// that unexpectedly reaches the store says which capability it wanted.
fn unused(method: &str) -> EntityError {
    EntityError::internal(format!("UnusedStore::{method} is not exercised by the read-path unit tests"))
}

#[async_trait]
impl EntityReader for UnusedStore {
    async fn get(&self, _target: &ContextTarget, _id: &Uri) -> Result<Option<Value>, EntityError> {
        Err(unused("get"))
    }

    async fn get_batch(&self, _target: &ContextTarget, _ids: &[Uri]) -> Result<HashMap<Uri, Value>, EntityError> {
        Err(unused("get_batch"))
    }

    async fn get_snapshot(&self, _target: &ContextTarget, _id: &Uri) -> Result<Option<EntitySnapshot>, EntityError> {
        Err(unused("get_snapshot"))
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Uri]) -> Result<HashMap<Uri, EntitySnapshot>, EntityError> {
        Err(unused("get_snapshot_batch"))
    }

    async fn get_sys_attrs(&self, _target: &ContextTarget, _id: &Uri) -> Result<Option<SysAttrsSnapshot>, EntityError> {
        Err(unused("get_sys_attrs"))
    }

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, _ids: &[Uri]) -> Result<HashMap<Uri, SysAttrsSnapshot>, EntityError> {
        Err(unused("get_sys_attrs_batch"))
    }
}

#[async_trait]
impl EntityQueryStore for UnusedStore {
    fn profile(&self) -> &StorageProfile {
        panic!("the read-path unit tests never inspect the store's profile")
    }

    async fn query_compiled(&self, _target: &ContextTarget, _compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Value>, EntityError> {
        Err(unused("query_compiled"))
    }

    async fn query_ids(&self, _target: &ContextTarget, _compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Uri>, EntityError> {
        Err(unused("query_ids"))
    }
}

/// An [`EntityQueryService`] with a live JSON-LD engine and a store that is
/// never called.
///
/// Remote context resolution is disabled, so a test context has to be an
/// inline body context for its terms to expand.
pub(crate) fn service() -> EntityQueryService {
    service_narrowing_through(SpatialAcceleration::StoreVerified)
}

/// The [`service`] test double, narrowing geo queries through `spatial`.
pub(crate) fn service_narrowing_through(spatial: SpatialAcceleration) -> EntityQueryService {
    let store = Arc::new(UnusedStore);
    EntityQueryService::new(
        store.clone() as Arc<dyn EntityReader>, // clone: the test double supplies both focused capabilities
        store as Arc<dyn EntityQueryStore>,
        JsonLdEngine::builder().allow_remote(false).build().expect("build engine"),
        spatial,
        TenantRef::UnnamedDefault,
        None,
    )
}
