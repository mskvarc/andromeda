//! Connection and capability holder for the PostgreSQL temporal repository.

use crate::bulk_write_mode::BulkWriteMode;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_collation::provisioned_collations::ProvisionedCollations;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use sqlx::postgres::PgPool;
use std::sync::Arc;

/// PostgreSQL/TimescaleDB-backed repository for NGSI-LD temporal entity data.
#[derive(Debug)]
pub struct TemporalRepository {
    /// Tenant connection pool every temporal statement runs on.
    pub(crate) pool: PgPool,
    /// Shared `attribute_names` dictionary cache.
    pub(crate) attribute_names: Arc<dyn NameLookup<AttributeName>>,
    /// Shared `entity_type_names` dictionary cache.
    pub(crate) entity_type_names: Arc<dyn NameLookup<EntityTypeName>>,
    /// Dispatch mode for bulk history inserts.
    pub(crate) bulk_write_mode: BulkWriteMode,
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
    /// Create a new repository backed by the given connection pool.
    pub fn new(
        pool: &PgPool,
        bulk_write_mode: BulkWriteMode,
        attribute_names: Arc<dyn NameLookup<AttributeName>>,
        entity_type_names: Arc<dyn NameLookup<EntityTypeName>>,
    ) -> TemporalRepository {
        TemporalRepository {
            pool: pool.clone(), // clone: `PgPool` shares its inner pool by reference count.
            attribute_names,
            entity_type_names,
            bulk_write_mode,
            collations: ProvisionedCollations::of_pool(pool),
        }
    }
}
