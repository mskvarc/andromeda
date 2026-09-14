//! Typed selection of live or Snapshot context state.

use crate::snapshot_target::SnapshotTarget;

/// Context branch selected for one Core, Temporal, or subscription operation.
///
/// `Hash`/`Eq` make the target a branch-keyed map key directly, which the
/// subscription cache and other per-branch indices rely on now that a branch is
/// no longer named by an integer key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContextTarget {
    /// The tenant's ordinary live branch, held in the default schema.
    Live,
    /// An isolated Snapshot branch, held in its own schema.
    Snapshot(SnapshotTarget),
}

impl ContextTarget {
    /// Return whether distributed discovery and forwarding are allowed.
    ///
    /// A Snapshot is an isolated point-in-time branch (clause 5.2.22): it never
    /// forwards to context sources, so only the live branch permits federation.
    #[must_use]
    pub const fn permits_federation(&self) -> bool {
        matches!(self, ContextTarget::Live)
    }

    /// Return the Snapshot target when the selected branch is Snapshot-owned.
    #[must_use]
    pub const fn snapshot(&self) -> Option<&SnapshotTarget> {
        match self {
            ContextTarget::Live => None,
            ContextTarget::Snapshot(target) => Some(target),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};

    #[test]
    fn the_live_branch_permits_federation_and_owns_no_snapshot() {
        assert!(ContextTarget::Live.permits_federation());
        assert!(ContextTarget::Live.snapshot().is_none());
    }

    #[test]
    fn a_snapshot_branch_forbids_federation_and_exposes_its_target() {
        let key = SnapshotKey::try_from(3).expect("positive key");
        let id = "urn:ngsi-ld:Snapshot:abc".parse().expect("valid snapshot uri");
        let target = ContextTarget::Snapshot(SnapshotTarget::published(key, id));
        assert!(!target.permits_federation());
        pretty_assertions::assert_eq!(target.snapshot().map(SnapshotTarget::key), Some(key));
    }
}
