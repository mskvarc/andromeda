//! Whether a Snapshot's stored branch can be served by this binary.

/// Whether a Snapshot's branch carries every branch migration the running
/// binary embeds.
///
/// A store upgrades each Snapshot's branch when the binary appends a branch
/// migration; a Snapshot whose upgrade has not run or has failed is
/// quarantined. The tenant keeps serving: only that Snapshot is refused for
/// scoping, cloning and notification delivery, and it is still reaped by
/// expiry, deletion and quota eviction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BranchCondition {
    /// The branch is at the binary's branch version.
    Current,
    /// The branch is behind the binary's branch version.
    Quarantined,
}
