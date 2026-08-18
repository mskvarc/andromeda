//! The physical storage policy an operator sets for temporal history.

use std::num::NonZeroU32;

/// How temporal history is laid out on disk over time.
///
/// Both values are day counts an operator may override; an absent value keeps
/// whatever the schema declares as its default. The backend reconciles the
/// stored policy against this one when a tenant is opened, so a change takes
/// effect without a migration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalStoragePolicy {
    /// The span one write-side partition of history covers.
    pub chunk_interval_days: Option<NonZeroU32>,
    /// The age past which a partition is converted to its compressed,
    /// column-oriented form; history younger than this is expected to be
    /// mutated in place, history older is read as an archive.
    pub columnstore_after_days: Option<NonZeroU32>,
}
