//! The physical storage policy an operator sets for temporal history.

use std::num::NonZeroU32;

/// How temporal history is laid out on disk over time.
///
/// Every value is a day count. The chunk interval is an override an operator
/// may set, and an absent value keeps the interval the schema declares. The
/// columnstore lag is always stated: no migration configures the columnstore,
/// so the backend enables it, where the edition has one, under this lag. The
/// backend reconciles the stored policy against this one when a tenant is
/// opened, so a change takes effect without a migration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalStoragePolicy {
    /// The span one write-side partition of history covers.
    pub chunk_interval_days: Option<NonZeroU32>,
    /// The age past which a partition is converted to its compressed,
    /// column-oriented form; history younger than this is expected to be
    /// mutated in place, history older is read as an archive.
    pub columnstore_after_days: NonZeroU32,
    /// The age past which a partition is dropped, absent while retention is
    /// off.
    pub retention_after_days: Option<NonZeroU32>,
}
