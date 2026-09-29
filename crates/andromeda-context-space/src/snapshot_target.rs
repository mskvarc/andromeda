//! Snapshot-owned context targets.

use crate::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// A validated Snapshot identifier paired with the branch it addresses.
///
/// The `key` names the Snapshot's branch schemas and the `branch` selects the
/// published or staging one; `snapshot_id` is the exact validated Snapshot URI
/// a notification carries in its `NGSILD-Snapshot` header.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SnapshotTarget {
    key: SnapshotKey,
    branch: SnapshotBranch,
    snapshot_id: Identifier,
}

impl SnapshotTarget {
    /// Target the Snapshot's published branch, the one scoping resolves.
    #[must_use]
    pub const fn published(key: SnapshotKey, snapshot_id: Identifier) -> SnapshotTarget {
        SnapshotTarget {
            key,
            branch: SnapshotBranch::Published,
            snapshot_id,
        }
    }

    /// Target the Snapshot's private staging branch, which capture and clone write.
    #[must_use]
    pub const fn staging(key: SnapshotKey, snapshot_id: Identifier) -> SnapshotTarget {
        SnapshotTarget {
            key,
            branch: SnapshotBranch::Staging,
            snapshot_id,
        }
    }

    /// Return the Snapshot surrogate key that names the branch schemas.
    #[must_use]
    pub const fn key(&self) -> SnapshotKey {
        self.key
    }

    /// Return which branch of the Snapshot is addressed.
    #[must_use]
    pub const fn branch(&self) -> SnapshotBranch {
        self.branch
    }

    /// Return the exact validated Snapshot URI.
    #[must_use]
    pub const fn snapshot_id(&self) -> &Identifier {
        &self.snapshot_id
    }
}
