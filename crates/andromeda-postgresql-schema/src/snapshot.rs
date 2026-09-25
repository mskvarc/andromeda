//! The relations a distributed-operation snapshot occupies.
//!
//! A Snapshot's control record is split by write pattern: `snapshots` holds the
//! immutable identity and query definition, `snapshot_preparation` holds the
//! lifecycle status and the renewable worker lease that every preparation tick
//! rewrites. Captured data lives in the Snapshot's own branch schema named after
//! its `snapshot_key`, not in these tables.

use sea_query::Iden;

/// The `snapshots` table - the immutable control record of one Snapshot.
#[derive(Iden, Clone, Copy)]
pub enum Snapshots {
    /// Table name: `snapshots`
    Table,
    /// The database-issued surrogate the branch schema and children are named from.
    SnapshotKey,
    /// The snapshot's URI identifier.
    Id,
    /// The entity queries the snapshot materialised.
    SnapshotQueries,
    /// The temporal queries the snapshot materialised.
    SnapshotTemporalQueries,
    /// The durable JSON-LD context used by preparation workers.
    QueryContext,
    /// The snapshot's lifetime as an ISO 8601 duration.
    SnapshotLifetime,
    /// The snapshot's scheduling priority.
    SnapshotPriority,
    /// The endpoint the snapshot is delivered to.
    Endpoint,
    /// The receiver's callback metadata.
    ReceiverInfo,
    /// Source Snapshot URI for an asynchronous clone preparation.
    CloneSourceId,
    /// Creation instant (clause 4.8 system attribute).
    CreatedAt,
    /// Expiry instant, after which the snapshot is reaped.
    ExpiresAt,
}

/// The `snapshot_preparation` table - the mutable lifecycle and lease state of
/// one Snapshot, keyed by `snapshot_key`.
#[derive(Iden, Clone, Copy)]
pub enum SnapshotPreparation {
    /// Table name: `snapshot_preparation`
    Table,
    /// Foreign key to `snapshots`, cascading on deletion; also the primary key.
    SnapshotKey,
    /// Last-modification instant (clause 4.8 system attribute).
    ModifiedAt,
    /// Instant the snapshot was last read.
    LastUsedAt,
    /// Instant a preparer claimed this record; `NULL` while unclaimed.
    ClaimedAt,
    /// Instant after which another replica may reclaim preparation.
    LeaseUntil,
    /// Identity of the replica currently preparing this Snapshot.
    ClaimedBy,
    /// The highest branch migration the Snapshot's schemas carry.
    BranchSchemaVersion,
    /// The snapshot's lifecycle status code.
    Status,
    /// Why the last branch upgrade failed, while the Snapshot is quarantined.
    BranchUpgradeError,
    /// Per-query execution details for the entity queries.
    QueriesDetails,
    /// Per-query execution details for the temporal queries.
    TemporalQueriesDetails,
}

/// The durable Snapshot lifecycle-notification delivery queue.
#[derive(Iden, Clone, Copy)]
pub enum SnapshotNotificationOutbox {
    /// Table name: `snapshot_notification_outbox`.
    Table,
    /// Durable delivery identity.
    Id,
    /// Parent Snapshot surrogate key.
    SnapshotKey,
    /// Creation instant.
    CreatedAt,
    /// Earliest instant at which another attempt is due.
    NextAttemptAt,
    /// Instant the current lease was acquired.
    ClaimedAt,
    /// Current lease deadline.
    LeaseUntil,
    /// Current worker identity.
    ClaimedBy,
    /// Completed attempt count.
    AttemptCount,
    /// Successful delivery instant.
    DeliveredAt,
    /// Dead-letter instant.
    ExhaustedAt,
    /// Callback endpoint.
    Endpoint,
    /// Receiver-provided HTTP metadata.
    ReceiverInfo,
    /// Standards-shaped notification body.
    Payload,
    /// Most recent transport error.
    LastError,
}

#[cfg(test)]
mod tests {
    use crate::snapshot::{SnapshotNotificationOutbox, SnapshotPreparation, Snapshots};
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn snapshot_relation_names() {
        assert_eq!(Snapshots::Table.to_string(), "snapshots");
        assert_eq!(Snapshots::SnapshotKey.to_string(), "snapshot_key");
        assert_eq!(Snapshots::Id.to_string(), "id");
        assert_eq!(Snapshots::CloneSourceId.to_string(), "clone_source_id");
        assert_eq!(Snapshots::ExpiresAt.to_string(), "expires_at");
        assert_eq!(SnapshotNotificationOutbox::Table.to_string(), "snapshot_notification_outbox");
    }

    #[test]
    fn snapshot_preparation_relation_names() {
        assert_eq!(SnapshotPreparation::Table.to_string(), "snapshot_preparation");
        assert_eq!(SnapshotPreparation::SnapshotKey.to_string(), "snapshot_key");
        assert_eq!(SnapshotPreparation::LastUsedAt.to_string(), "last_used_at");
        assert_eq!(SnapshotPreparation::ClaimedAt.to_string(), "claimed_at");
        assert_eq!(SnapshotPreparation::ClaimedBy.to_string(), "claimed_by");
        assert_eq!(SnapshotPreparation::LeaseUntil.to_string(), "lease_until");
        assert_eq!(SnapshotPreparation::Status.to_string(), "status");
    }
}
