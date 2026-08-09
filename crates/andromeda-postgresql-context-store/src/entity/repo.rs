//! The PostgreSQL entity store.
//!
//! Holds the tenant pool plus the two things a pushed-down `q` predicate needs:
//! the attribute-name dictionary the projection is keyed on, and the barrier
//! that says the projection has caught up. Both are properties of this store,
//! not of the policy above it, which is why they live here and not in the port.

use andromeda_entity::storage_profile::StorageProfile;
use andromeda_postgresql_query::{attribute_name_lookup::AttributeNameLookup, projection_barrier::ProjectionBarrier};
use sqlx::postgres::PgPool;
use std::sync::Arc;

/// Repository for persisting and querying NGSI-LD entities.
#[derive(Debug)]
pub struct EntityRepository {
    /// The tenant's connection pool.
    pub(super) pool: PgPool,
    /// The attribute-name dictionary, shared with the temporal store.
    pub(super) attribute_names: Arc<dyn AttributeNameLookup>,
    /// The barrier that says the `attribute_current` projection has caught up.
    pub(super) attribute_current: Arc<dyn ProjectionBarrier>,
}

impl EntityRepository {
    /// What this store can be asked to do with a compiled query.
    ///
    /// A constant rather than a probe: `migrations/tenant/` creates the PostGIS
    /// geometry columns and the `ltree` scope columns, so a pool that migrated
    /// has them and one that could not migrate never becomes a store. That is
    /// the difference between these axes and the TimescaleDB edition, which is
    /// genuinely per-pool runtime state.
    pub const PROFILE: StorageProfile = StorageProfile::POSTGIS_LTREE;

    /// Create a new entity repository backed by the given connection pool.
    pub fn new(pool: &PgPool, attribute_names: Arc<dyn AttributeNameLookup>, attribute_current: Arc<dyn ProjectionBarrier>) -> EntityRepository {
        EntityRepository {
            pool: pool.clone(), // clone: Arc ref-count bump
            attribute_names,
            attribute_current,
        }
    }
}
