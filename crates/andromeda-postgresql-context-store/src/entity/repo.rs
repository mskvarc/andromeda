//! The PostgreSQL entity store.
//!
//! Holds the tenant pool plus the two things a pushed-down predicate needs:
//! the attribute-name dictionary the current-value projection is keyed on and
//! the entity-type-name dictionary the `type_ids` column is keyed on. Those
//! are properties of this store, not of the policy above it, which is why they
//! live here and not in the port.
//!
//! Nothing here waits for the projection. It is written inside the same
//! transaction as the entity row it describes, so a committed write is a
//! visible write and a reader that can see the entity can see its projected
//! values.

use andromeda_entity::storage_profile::StorageProfile;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use sqlx::postgres::PgPool;
use std::sync::Arc;

/// Repository for persisting and querying NGSI-LD entities.
#[derive(Debug)]
pub struct EntityRepository {
    /// The tenant's connection pool.
    pub(super) pool: PgPool,
    /// The attribute-name dictionary, shared with the temporal store.
    pub(super) attribute_names: Arc<dyn NameLookup<AttributeName>>,
    /// The entity-type-name dictionary, shared with the temporal store.
    pub(super) entity_type_names: Arc<dyn NameLookup<EntityTypeName>>,
}

impl EntityRepository {
    /// What this store can be asked to do with a compiled query.
    ///
    /// A constant rather than a probe: `migrations/tenant/` creates the `PostGIS`
    /// geometry columns and the `ltree` scope columns, so a pool that migrated
    /// has them and one that could not migrate never becomes a store. That is
    /// the difference between these axes and the `TimescaleDB` edition, which is
    /// genuinely per-pool runtime state.
    pub const PROFILE: StorageProfile = StorageProfile::POSTGIS_LTREE;

    /// Create a new entity repository backed by the given connection pool.
    pub fn new(pool: &PgPool, attribute_names: Arc<dyn NameLookup<AttributeName>>, entity_type_names: Arc<dyn NameLookup<EntityTypeName>>) -> EntityRepository {
        EntityRepository {
            pool: pool.clone(), // clone: Arc ref-count bump
            attribute_names,
            entity_type_names,
        }
    }
}
