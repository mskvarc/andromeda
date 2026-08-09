//! Connection and capability holder for the PostgreSQL temporal repository.

use crate::{bulk_write_mode::BulkWriteMode, timescale_features::TimescaleFeatures};
use andromeda_postgresql_query::{attribute_name_lookup::AttributeNameLookup, projection_barrier::ProjectionBarrier};
use sqlx::postgres::PgPool;
use std::sync::Arc;

/// PostgreSQL/TimescaleDB-backed repository for NGSI-LD temporal entity data.
#[derive(Debug)]
pub struct TemporalRepository {
    pub(crate) pool: PgPool,
    /// Shared `attribute_names` dictionary cache.
    pub(crate) attribute_names: Arc<dyn AttributeNameLookup>,
    /// Barrier for pending `attribute_current` materialiser writes.
    pub(crate) attribute_current: Option<Arc<dyn ProjectionBarrier>>,
    /// TSL capability probe.
    pub(crate) timescale_features: TimescaleFeatures,
    /// Dispatch mode for bulk history inserts.
    pub(crate) bulk_write_mode: BulkWriteMode,
}

impl TemporalRepository {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool, bulk_write_mode: BulkWriteMode, attribute_names: Arc<dyn AttributeNameLookup>) -> TemporalRepository {
        TemporalRepository {
            pool: pool.clone(), // clone: `PgPool` shares its inner pool by reference count.
            attribute_names,
            attribute_current: None,
            timescale_features: TimescaleFeatures::empty(),
            bulk_write_mode,
        }
    }

    /// Attach the `attribute_current` materialiser barrier.
    pub fn with_barrier(mut self, barrier: Arc<dyn ProjectionBarrier>) -> TemporalRepository {
        self.attribute_current = Some(barrier);
        self
    }

    /// Replace the repository's TimescaleDB feature set.
    pub const fn with_features(mut self, features: TimescaleFeatures) -> TemporalRepository {
        self.timescale_features = features;
        self
    }
}
