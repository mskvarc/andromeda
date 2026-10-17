//! Connection and capability holder for the PostgreSQL temporal repository.

use crate::columnstore_capability::ColumnstoreSupport;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_collation::provisioned_collations::ProvisionedCollations;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_pool::read_share::ReadShare;
use sqlx::postgres::PgPool;
use std::sync::Arc;

/// PostgreSQL/TimescaleDB-backed repository for NGSI-LD temporal entity data.
#[derive(Debug)]
pub struct TemporalRepository {
    /// Tenant connection pool every temporal write, and every read a write
    /// makes, runs on.
    pub(crate) pool: PgPool,
    /// The connections of the pool a temporal query may hold.
    pub(crate) reads: ReadShare,
    /// Shared `attribute_names` dictionary cache.
    pub(crate) attribute_names: Arc<dyn NameLookup<AttributeName>>,
    /// Shared `entity_type_names` dictionary cache.
    pub(crate) entity_type_names: Arc<dyn NameLookup<EntityTypeName>>,
    /// Whether this tenant's `TimescaleDB` edition provides the hypercore
    /// columnstore, probed once when the tenant was opened.
    ///
    /// The storage-policy reconciler and the conversion sweep consult it so
    /// the columnstore is only enabled and converted to where the edition can
    /// hold one; the Apache-2.0 core runs the hypertables uncompressed.
    pub(crate) columnstore: ColumnstoreSupport,
    /// ICU collations already provisioned on this tenant's database.
    ///
    /// Clause 5.7.4.3 lets a temporal query order its entities under a client
    /// collation, and the object that collation names has to exist before the
    /// statement runs. The temporal read path can be the first to ask for one,
    /// so this store provisions on its own rather than relying on an entity
    /// query having asked first.
    pub(crate) collations: ProvisionedCollations,
}

impl TemporalRepository {
    /// Wraps `pool`, the read requests' share `reads` of it and the shared
    /// dictionary caches; the provisioned-collation set starts empty.
    pub fn new(
        pool: &PgPool,
        reads: ReadShare,
        attribute_names: Arc<dyn NameLookup<AttributeName>>,
        entity_type_names: Arc<dyn NameLookup<EntityTypeName>>,
        columnstore: ColumnstoreSupport,
    ) -> TemporalRepository {
        TemporalRepository {
            pool: pool.clone(), // clone: `PgPool` shares its inner pool by reference count.
            reads,
            attribute_names,
            entity_type_names,
            columnstore,
            collations: ProvisionedCollations::of_pool(pool),
        }
    }

    /// Whether this tenant's edition provides the columnstore.
    #[must_use]
    pub const fn columnstore(&self) -> ColumnstoreSupport {
        self.columnstore
    }
}
