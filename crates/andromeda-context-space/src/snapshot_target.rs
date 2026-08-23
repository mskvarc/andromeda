//! Snapshot-owned context targets.

use crate::context_space::{ContextSpace, ContextSpaceKind};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use thiserror::Error;

/// A validated Snapshot identifier paired with its durable context branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotTarget {
    space: ContextSpace,
    snapshot_id: Uri,
}

impl SnapshotTarget {
    /// Construct a target after resolving a Snapshot control row.
    pub fn new(space: ContextSpace, snapshot_id: Uri) -> Result<SnapshotTarget, SnapshotTargetError> {
        match space.kind() {
            ContextSpaceKind::Snapshot => Ok(SnapshotTarget { space, snapshot_id }),
            ContextSpaceKind::Live => Err(SnapshotTargetError::LiveSpace),
        }
    }

    /// Return the Snapshot-owned context space.
    pub const fn space(&self) -> ContextSpace {
        self.space
    }

    /// Return the exact validated Snapshot URI.
    pub const fn snapshot_id(&self) -> &Uri {
        &self.snapshot_id
    }
}

/// Failure to construct a Snapshot target from incompatible persisted state.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SnapshotTargetError {
    /// A live space cannot be owned by a Snapshot.
    #[error("a Snapshot target requires a Snapshot context space")]
    LiveSpace,
}
