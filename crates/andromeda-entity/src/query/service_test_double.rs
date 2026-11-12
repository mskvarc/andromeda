//! The [`EntityQueryService`] the read-path unit tests are built on.
//!
//! Several read-path modules exercise service methods that touch only the
//! JSON-LD engine, term expansion, projection compilation, nested-projection
//! derivation, and never read a row. They all need the same service wired to
//! a store that is never called, so that store lives here once instead of
//! being restated in each test module.

use crate::{
    configured_policy::{filter_complexity_limits, geometry_census_limits},
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    port::{query::EntityQueryStore, reader::EntityReader, retrieved_entity::RetrievedEntity, stored_entry::StoredEntry},
    query::EntityQueryService,
    query_outcome::QueryOutcome,
    spatial_acceleration::SpatialAcceleration,
    storage_profile::StorageProfile,
};
use andromeda_configuration::schema::request_limits::RequestLimitsConfig;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_geometry::geometry_census::GeometryCensusLimits;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    filter::language_targeting::LanguageTargeting,
    geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand},
    page_window::PageWindow,
    projection::returned_attributes::ReturnedAttributes,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};

/// A store whose every method is an error: the tests built on it never read.
struct UnusedStore;

/// The error a call into [`UnusedStore`] returns, naming the method so a test
/// that unexpectedly reaches the store says which capability it wanted.
fn unused(method: &str) -> EntityError {
    panic!("UnusedStore::{method} is not exercised by the read-path unit tests")
}

#[async_trait]
impl EntityReader for UnusedStore {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
        Err(unused("get"))
    }

    async fn get_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
        Err(unused("get_batch"))
    }

    async fn get_with_sys_attrs(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _returned: &ReturnedAttributes<'_>,
    ) -> Result<Option<RetrievedEntity>, EntityError> {
        Err(unused("get_with_sys_attrs"))
    }

    async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        Err(unused("get_snapshot"))
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        Err(unused("get_snapshot_batch"))
    }

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        Err(unused("get_sys_attrs_batch"))
    }
}

#[async_trait]
impl EntityQueryStore for UnusedStore {
    fn profile(&self) -> &StorageProfile {
        panic!("the read-path unit tests never inspect the store's profile")
    }

    async fn query_compiled(
        &self,
        _target: &ContextTarget,
        _compiled: &CompiledEntityQuery,
        _window: &PageWindow,
        _count: CountPolicy,
    ) -> Result<QueryOutcome<StoredEntry<RetrievedEntity>>, EntityError> {
        Err(unused("query_compiled"))
    }

    async fn query_ids(&self, _target: &ContextTarget, _compiled: &CompiledEntityQuery, _window: &PageWindow) -> Result<QueryOutcome<Identifier>, EntityError> {
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
    service_with(spatial, geometry_census_limits(&RequestLimitsConfig::default()))
}

/// The [`service`] test double, resolving reference geometries of at most
/// what `reference_limits` admits.
pub(crate) fn service_resolving(reference_limits: GeometryCensusLimits) -> EntityQueryService {
    service_with(SpatialAcceleration::StoreVerified, reference_limits)
}

/// The [`service`] test double, expanding through `engine`.
pub(crate) fn service_expanding_through(engine: JsonLdEngine) -> EntityQueryService {
    service_over(
        SpatialAcceleration::StoreVerified,
        geometry_census_limits(&RequestLimitsConfig::default()),
        engine,
    )
}

/// The test double narrowing through `spatial` and resolving reference
/// geometries within `reference_limits`.
fn service_with(spatial: SpatialAcceleration, reference_limits: GeometryCensusLimits) -> EntityQueryService {
    service_over(
        spatial,
        reference_limits,
        JsonLdEngine::builder().allow_remote(false).build().expect("build engine"),
    )
}

/// The test double narrowing through `spatial`, resolving reference
/// geometries within `reference_limits` and expanding through `engine`.
fn service_over(spatial: SpatialAcceleration, reference_limits: GeometryCensusLimits, engine: JsonLdEngine) -> EntityQueryService {
    let store = Arc::new(UnusedStore);
    EntityQueryService::builder()
        .reader(store.clone() as Arc<dyn EntityReader>)
        .query_store(store as Arc<dyn EntityQueryStore>)
        .engine(engine)
        .spatial(spatial)
        .tenant(TenantRef::UnnamedDefault)
        .remote_linked(None)
        .geo_admission(GeoQueryAdmission::new(reference_limits, NearDistanceBand::Refused))
        .language_targeting(LanguageTargeting::Specification)
        .filter_complexity(filter_complexity_limits(&RequestLimitsConfig::default()))
        .build()
}
