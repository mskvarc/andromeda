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
    /// Names the Snapshot's branch schemas.
    key: SnapshotKey,
    /// Whether the published or the staging branch is addressed.
    branch: SnapshotBranch,
    /// The validated Snapshot URI a notification carries in its `NGSILD-Snapshot` header.
    snapshot_id: Identifier,
}

impl SnapshotTarget {
    /// Target the branch `branch` of the Snapshot.
    // Written out: the `derive_more::Constructor` expansion for named fields
    // spells every field as `field: field`, which `clippy::redundant_field_names`
    // refuses.
    #[must_use]
    pub const fn new(key: SnapshotKey, branch: SnapshotBranch, snapshot_id: Identifier) -> SnapshotTarget {
        SnapshotTarget { key, branch, snapshot_id }
    }

    /// Target the Snapshot's published branch, the one scoping resolves.
    #[must_use]
    pub const fn published(key: SnapshotKey, snapshot_id: Identifier) -> SnapshotTarget {
        SnapshotTarget::new(key, SnapshotBranch::Published, snapshot_id)
    }

    /// Target the Snapshot's private staging branch, which capture and clone write.
    #[must_use]
    pub const fn staging(key: SnapshotKey, snapshot_id: Identifier) -> SnapshotTarget {
        SnapshotTarget::new(key, SnapshotBranch::Staging, snapshot_id)
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

#[cfg(test)]
mod tests {
    use crate::{snapshot_branch::SnapshotBranch, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;

    fn snapshot_id() -> Identifier {
        "urn:ngsi-ld:Snapshot:abc".parse().expect("a valid snapshot uri")
    }

    #[test]
    fn a_target_built_from_a_branch_addresses_the_branch_its_named_constructor_does() {
        let key = SnapshotKey::try_from(7).expect("a positive key");
        assert_eq!(
            SnapshotTarget::new(key, SnapshotBranch::Published, snapshot_id()),
            SnapshotTarget::published(key, snapshot_id())
        );
        assert_eq!(
            SnapshotTarget::new(key, SnapshotBranch::Staging, snapshot_id()),
            SnapshotTarget::staging(key, snapshot_id())
        );
    }
}
