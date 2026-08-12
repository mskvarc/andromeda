//! Connection and capability holder for the PostgreSQL temporal repository.

use crate::bulk_write_mode::BulkWriteMode;
use andromeda_postgresql_attribute_dictionary::attribute_name_lookup::AttributeNameLookup;
use sqlx::postgres::PgPool;
use std::sync::Arc;

/// PostgreSQL/TimescaleDB-backed repository for NGSI-LD temporal entity data.
#[derive(Debug)]
pub struct TemporalRepository {
    /// Tenant connection pool every temporal statement runs on.
    pub(crate) pool: PgPool,
    /// Shared `attribute_names` dictionary cache.
    pub(crate) attribute_names: Arc<dyn AttributeNameLookup>,
    /// Dispatch mode for bulk history inserts.
    pub(crate) bulk_write_mode: BulkWriteMode,
}

impl TemporalRepository {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool, bulk_write_mode: BulkWriteMode, attribute_names: Arc<dyn AttributeNameLookup>) -> TemporalRepository {
        TemporalRepository {
            pool: pool.clone(), // clone: `PgPool` shares its inner pool by reference count.
            attribute_names,
            bulk_write_mode,
        }
    }
}
