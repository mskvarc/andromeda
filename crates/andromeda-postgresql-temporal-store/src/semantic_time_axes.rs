//! The NGSI-LD time axes a stored temporal row carries besides its write clock.

use chrono::{DateTime, Utc};

/// The semantic timestamps one temporal history row records.
///
/// Clause 5.2.21 admits `observedAt`, `createdAt`, `modifiedAt` and `deletedAt`
/// as the `timeproperty` a temporal query may select, and clause 4.11 makes
/// `observedAt` the default. Each is stored in its own nullable column and a
/// filter reads exactly the column it names: a row that was written without an
/// `observedAt` has none, and must therefore match no `observedAt` filter. The
/// operational `recorded_at` the hypertable partitions on is deliberately not
/// part of this type — it is never rendered as a Temporal Property, and folding
/// it in as a fallback is what would invent the match the clause forbids.
///
/// `deletedAt` is absent here because it is not independent: a row carries a
/// deletion timestamp exactly when it records a deletion, so it travels inside
/// the tombstone value rather than beside it.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct SemanticTimeAxes {
    /// The instant the client declared the observation was made at.
    pub observed_at: Option<DateTime<Utc>>,
    /// The instant the broker first recorded this attribute instance.
    pub created_at: Option<DateTime<Utc>>,
    /// The instant the broker last altered this attribute instance.
    pub modified_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use crate::semantic_time_axes::SemanticTimeAxes;

    #[test]
    fn a_row_written_without_semantic_timestamps_carries_none_of_them() {
        let axes = SemanticTimeAxes::default();
        assert!(axes.observed_at.is_none());
        assert!(axes.created_at.is_none());
        assert!(axes.modified_at.is_none());
    }
}
