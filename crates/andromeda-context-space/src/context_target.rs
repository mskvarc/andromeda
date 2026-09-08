//! Typed selection of live or Snapshot context state.

use crate::{
    context_space::{ContextSpace, ContextSpaceKind},
    context_space_key::ContextSpaceKey,
    snapshot_target::SnapshotTarget,
};
use thiserror::Error;

/// Context branch selected for one Core, Temporal, or subscription operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextTarget {
    /// The tenant's ordinary live branch.
    Live(ContextSpace),
    /// An isolated Snapshot branch.
    Snapshot(SnapshotTarget),
}

impl ContextTarget {
    /// Construct a live target from the tenant's persisted live space.
    pub const fn live(space: ContextSpace) -> Result<ContextTarget, ContextTargetError> {
        match space.kind() {
            ContextSpaceKind::Live => Ok(ContextTarget::Live(space)),
            ContextSpaceKind::Snapshot => Err(ContextTargetError::SnapshotSpace),
        }
    }

    /// Return the selected durable branch key.
    #[must_use]
    pub const fn key(&self) -> ContextSpaceKey {
        match self {
            ContextTarget::Live(space) => space.key(),
            ContextTarget::Snapshot(target) => target.space().key(),
        }
    }

    /// Return the selected branch role.
    #[must_use]
    pub const fn kind(&self) -> ContextSpaceKind {
        match self {
            ContextTarget::Live(space) => space.kind(),
            ContextTarget::Snapshot(target) => target.space().kind(),
        }
    }

    /// Return whether distributed discovery and forwarding are allowed.
    #[must_use]
    pub const fn permits_federation(&self) -> bool {
        match self {
            ContextTarget::Live(_) => true,
            ContextTarget::Snapshot(_) => false,
        }
    }

    /// Return the Snapshot target when the selected branch is Snapshot-owned.
    #[must_use]
    pub const fn snapshot(&self) -> Option<&SnapshotTarget> {
        match self {
            ContextTarget::Live(_) => None,
            ContextTarget::Snapshot(target) => Some(target),
        }
    }
}

/// Failure to construct a live target from incompatible persisted state.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContextTargetError {
    /// A Snapshot space cannot serve as the tenant's live target.
    #[error("a live target requires a live context space")]
    SnapshotSpace,
}

#[cfg(test)]
mod tests {
    use crate::{
        context_space::{ContextSpace, ContextSpaceKind},
        context_space_key::ContextSpaceKey,
        context_target::{ContextTarget, ContextTargetError},
    };

    #[test]
    fn live_target_preserves_key_and_federation_policy() {
        let key = ContextSpaceKey::try_from(3).expect("positive key");
        let target = ContextTarget::live(ContextSpace::new(key, ContextSpaceKind::Live)).expect("live space");
        pretty_assertions::assert_eq!(target.key(), key);
        assert!(target.permits_federation());
        assert!(target.snapshot().is_none());
    }

    #[test]
    fn snapshot_space_cannot_be_constructed_as_live() {
        let key = ContextSpaceKey::try_from(4).expect("positive key");
        pretty_assertions::assert_eq!(
            ContextTarget::live(ContextSpace::new(key, ContextSpaceKind::Snapshot)),
            Err(ContextTargetError::SnapshotSpace)
        );
    }
}
