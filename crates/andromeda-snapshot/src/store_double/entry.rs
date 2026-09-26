//! One Snapshot the store double holds, and where its preparation stands.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::snapshot::{Snapshot, SnapshotStatus};

/// Where one stored Snapshot's preparation stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Stage {
    /// Preparing, and not claimed by any worker.
    Waiting,
    /// Preparing under a worker's claim.
    Claimed,
    /// Out of `preparing`, in this status.
    Ended(SnapshotStatus),
}

/// One stored Snapshot.
pub(crate) struct Entry {
    /// The Snapshot.
    pub(crate) snapshot: Snapshot,
    /// The source it clones, for a clone job.
    pub(crate) clone_source: Option<Uri>,
    /// Where its preparation stands.
    pub(crate) stage: Stage,
}

impl Entry {
    /// A stored Snapshot whose preparation stands where its status says.
    pub(crate) fn new(snapshot: Snapshot, clone_source: Option<Uri>) -> Entry {
        let stage = match snapshot.status {
            SnapshotStatus::Preparing => Stage::Waiting,
            status @ (SnapshotStatus::Success | SnapshotStatus::Partial | SnapshotStatus::Empty | SnapshotStatus::Failure) => Stage::Ended(status),
        };
        Entry { snapshot, clone_source, stage }
    }

    /// The status the Snapshot currently has.
    pub(crate) const fn status(&self) -> SnapshotStatus {
        match self.stage {
            Stage::Waiting | Stage::Claimed => SnapshotStatus::Preparing,
            Stage::Ended(status) => status,
        }
    }
}
