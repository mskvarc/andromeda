//! The two schema branches a Snapshot addresses.

/// Which of a Snapshot's two branches an operation targets.
///
/// A Snapshot owns a published branch, resolved by scoping once preparation
/// completes, and a private staging branch that capture and clone workers write
/// before promotion swaps it in. The PostgreSQL backend realises the two as the
/// schemas `snap_<key>` and `snap_<key>_stg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapshotBranch {
    /// The published branch clients read once the Snapshot is ready.
    Published,
    /// The private preparation branch capture and clone write.
    Staging,
}
