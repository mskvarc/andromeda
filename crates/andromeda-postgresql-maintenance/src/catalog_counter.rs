//! The PostgreSQL counters the maintenance controller reads and records.

use crate::error::MaintenanceError;
use strum::Display;

/// One counter read from a PostgreSQL catalog or statistics view, or written
/// back to the platform maintenance state.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub enum CatalogCounter {
    /// Cheap catalog size of an allowlisted index.
    IndexBytes,
    /// Estimated live tuples of an allowlisted heap.
    LiveTuples,
    /// Estimated dead tuples of an allowlisted heap.
    DeadTuples,
    /// Total relation size of an allowlisted heap.
    HeapBytes,
    /// Pages an allowlisted index holds, from `pg_class`.
    IndexPages,
    /// Entries an allowlisted index holds, from `pg_class`.
    IndexTuples,
    /// Average key width of an allowlisted index, from `pg_stats`.
    IndexKeyWidth,
    /// The server's page size.
    BlockSize,
    /// Rows a density-measured GIN covers, from `pg_class` and `pg_stats`.
    CoveredRows,
    /// Free bytes a detailed index probe measured.
    IndexFreeBytes,
    /// Physical length a detailed index probe measured.
    IndexPhysicalBytes,
    /// Active foreground backends.
    ActiveBackends,
    /// Configured maximum connections.
    MaximumConnections,
    /// Current physical size of a maintenance target.
    RelationBytes,
    /// Total size of an allowlisted base table.
    TableTotalBytes,
    /// Index size of an allowlisted base table.
    TableIndexBytes,
    /// `n_tup_upd` of an allowlisted base table.
    TableUpdatedTuples,
    /// `n_tup_hot_upd` of an allowlisted base table.
    TableHotUpdatedTuples,
    /// Total size of a history hypertable's largest chunk not yet converted.
    RowstoreChunkBytes,
    /// Samples in a target's recorded breach run.
    BreachSamples,
    /// Consecutive failed actions recorded for a target.
    ConsecutiveFailures,
    /// Age in transactions of an ID holding the vacuum horizon.
    TransactionAge,
    /// Transaction or multixact ID age of a database's oldest unfrozen ID.
    DatabaseFreezeAge,
    /// A server's anti-wraparound freeze maximum setting.
    FreezeMaxAge,
    /// Transaction or multixact ID age of a relation's oldest unfrozen ID.
    RelationFreezeAge,
    /// Rows a database, or the tables of one Snapshot branch, updated or
    /// deleted since their statistics were reset.
    TupleChanges,
    /// Relation bytes of a recorded degradation sample.
    SampleRelationBytes,
    /// Reclaimable bytes of a recorded degradation sample.
    SampleReclaimableBytes,
    /// Relation bytes recorded before a maintenance action.
    BeforeBytes,
    /// Relation bytes recorded after a maintenance action.
    AfterBytes,
    /// Dead tuples recorded before a vacuum.
    BeforeDeadTuples,
    /// Dead tuples recorded after a vacuum.
    AfterDeadTuples,
    /// A density-measured GIN's recorded baseline density.
    BaselineDensity,
    /// Dead tuples past which autovacuum vacuums a table.
    AutovacuumTrigger,
    /// Autovacuum workers running across the cluster.
    AutovacuumWorkers,
    /// The server's `autovacuum_max_workers` setting.
    AutovacuumMaxWorkers,
    /// Consecutive passes that found every autovacuum worker slot busy.
    SaturatedPasses,
    /// Tuples modified since a table's last analyze past which autovacuum
    /// analyzes it.
    AnalyzeTrigger,
    /// `n_mod_since_analyze`: tuples a table modified since its last analyze.
    ModifiedSinceAnalyze,
    /// History chunks still in the rowstore past the columnstore horizon.
    OverdueChunks,
    /// Runs of a `TimescaleDB` job that failed.
    JobFailures,
    /// WAL bytes a replication slot keeps, or may still take before it is
    /// lost.
    SlotWalBytes,
    /// Partially converted chunks of a history hypertable.
    PartialChunks,
    /// Bytes of a converted chunk's own heap, where its rows outside the
    /// compressed batches live.
    UncompressedBytes,
    /// The OID of a history chunk.
    ChunkOid,
    /// Closed rowstore history chunks whose indexes await their rebuild.
    AwaitingChunks,
    /// Bytes of every index of a history chunk.
    ChunkIndexBytes,
    /// Entries `pg_stat_statements` evicted, or the entries it may hold.
    StatementEvictions,
}

/// Convert one PostgreSQL counter that the catalog guarantees is non-negative.
pub(crate) fn non_negative(value: i64, counter: CatalogCounter) -> Result<u64, MaintenanceError> {
    u64::try_from(value).map_err(|_| MaintenanceError::NegativeCatalogValue { counter, value })
}

/// Convert an object identifier PostgreSQL reported as a `bigint`.
pub(crate) fn object_id(value: i64, counter: CatalogCounter) -> Result<u32, MaintenanceError> {
    u32::try_from(value).map_err(|_| MaintenanceError::NegativeCatalogValue { counter, value })
}

/// Convert a count to the platform schema's signed `bigint` storage type.
pub(crate) fn persisted_count(value: u64, counter: CatalogCounter) -> Result<i64, MaintenanceError> {
    i64::try_from(value).map_err(|_| MaintenanceError::CatalogValueOverflow { counter, value })
}

/// Compute a saturating integer percentage with bounded cardinality.
pub(crate) fn percent(numerator: u64, denominator: u64) -> u8 {
    let value = numerator.saturating_mul(100).checked_div(denominator).unwrap_or_default();
    u8::try_from(Ord::min(value, 100)).unwrap_or(100)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        catalog_counter::{CatalogCounter, non_negative, percent},
        error::MaintenanceError,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn percentage_is_integer_and_bounded() {
        assert_eq!(percent(0, 10), 0);
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(20, 10), 100);
    }

    #[test]
    fn a_negative_counter_names_the_counter_it_came_from() {
        let error = non_negative(-1, CatalogCounter::DeadTuples).expect_err("a negative counter is refused");
        assert!(matches!(
            error,
            MaintenanceError::NegativeCatalogValue {
                counter: CatalogCounter::DeadTuples,
                value: -1
            }
        ));
        assert_eq!(error.to_string(), "PostgreSQL catalog returned a negative value for dead-tuples: -1");
    }
}
