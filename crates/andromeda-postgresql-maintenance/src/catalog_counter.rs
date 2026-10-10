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
}

/// Convert one PostgreSQL counter that the catalog guarantees is non-negative.
pub(crate) fn non_negative(value: i64, counter: CatalogCounter) -> Result<u64, MaintenanceError> {
    u64::try_from(value).map_err(|_| MaintenanceError::NegativeCatalogValue { counter, value })
}

/// Compute a saturating integer percentage with bounded cardinality.
pub(crate) fn percent(numerator: u64, denominator: u64) -> u8 {
    let value = numerator.saturating_mul(100).checked_div(denominator).unwrap_or_default();
    u8::try_from(Ord::min(value, 100)).unwrap_or(100)
}

#[cfg(test)]
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
