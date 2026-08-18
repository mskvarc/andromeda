//! Connection and capability holder for the PostgreSQL temporal repository.

use crate::bulk_write_mode::BulkWriteMode;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
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
        }
    }
}
