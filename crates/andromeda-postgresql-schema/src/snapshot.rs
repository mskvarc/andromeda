//! The relations a distributed-operation snapshot occupies.
//!
//! A `snapshots` row is the Snapshot control record. Captured data is stored in
//! ordinary current and temporal relations partitioned by its context space.

use sea_query::Iden;

/// The `snapshots` table - one control record per distributed-operation snapshot.
#[derive(Iden, Clone, Copy)]
pub enum Snapshots {
    /// Table name: `snapshots`
    Table,
    /// The database-issued surrogate the child tables reference.
    SnapshotKey,
    /// The role-neutral branch containing this Snapshot's data.
    ContextSpaceKey,
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
    /// The snapshot's lifecycle status enum.
    Status,
    /// Source Snapshot URI for an asynchronous clone preparation.
    CloneSourceId,
    /// Instant a preparer claimed this control record; `NULL` while unclaimed.
    ClaimedAt,
    /// Identity of the replica currently preparing this Snapshot.
    ClaimedBy,
    /// Instant after which another replica may reclaim preparation.
    LeaseUntil,
    /// Per-query execution details for the entity queries.
    QueriesDetails,
    /// Per-query execution details for the temporal queries.
    TemporalQueriesDetails,
    /// Creation instant (clause 4.8 system attribute).
    CreatedAt,
    /// Last-modification instant (clause 4.8 system attribute).
    ModifiedAt,
    /// Expiry instant, after which the snapshot is reaped.
    ExpiresAt,
    /// Instant the snapshot was last read.
    LastUsedAt,
}

#[cfg(test)]
mod tests {
    use crate::snapshot::Snapshots;
    use sea_query::Iden;

    #[test]
    fn snapshot_relation_names() {
        assert_eq!(Snapshots::Table.to_string(), "snapshots");
        assert_eq!(Snapshots::SnapshotKey.to_string(), "snapshot_key");
        assert_eq!(Snapshots::ContextSpaceKey.to_string(), "context_space_key");
        assert_eq!(Snapshots::LastUsedAt.to_string(), "last_used_at");
        assert_eq!(Snapshots::ClaimedAt.to_string(), "claimed_at");
        assert_eq!(Snapshots::ClaimedBy.to_string(), "claimed_by");
        assert_eq!(Snapshots::LeaseUntil.to_string(), "lease_until");
    }
}
