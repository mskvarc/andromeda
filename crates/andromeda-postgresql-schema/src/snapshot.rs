//! The relations a distributed-operation snapshot occupies.
//!
//! A `snapshots` row is the snapshot's control record; `snapshot_entities` and
//! `snapshot_temporal_entities` hold the materialised entity and temporal-entity
//! documents it captured, one row per entity, keyed by the snapshot's surrogate
//! key and the entity identifier. The surrogate is never rendered: every
//! statement that starts from the snapshot's URI resolves it with a scalar
//! subquery on the unique `id`.

use sea_query::Iden;

/// The `snapshots` table - one control record per distributed-operation snapshot.
#[derive(Iden, Clone, Copy)]
pub enum Snapshots {
    /// Table name: `snapshots`
    Table,
    /// The database-issued surrogate the child tables reference.
    SnapshotKey,
    /// The snapshot's URI identifier.
    Id,
    /// The entity queries the snapshot materialised.
    SnapshotQueries,
    /// The temporal queries the snapshot materialised.
    SnapshotTemporalQueries,
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

/// The `snapshot_entities` table - one materialised entity document per row.
#[derive(Iden, Clone, Copy)]
pub enum SnapshotEntities {
    /// Table name: `snapshot_entities`
    Table,
    /// Foreign key to the owning `snapshots` row's surrogate.
    SnapshotKey,
    /// The captured entity's URI identifier.
    EntityId,
    /// The materialised entity document.
    EntityData,
}

/// The `snapshot_temporal_entities` table - one materialised temporal-entity
/// document per row.
#[derive(Iden, Clone, Copy)]
pub enum SnapshotTemporalEntities {
    /// Table name: `snapshot_temporal_entities`
    Table,
    /// Foreign key to the owning `snapshots` row's surrogate.
    SnapshotKey,
    /// The captured entity's URI identifier.
    EntityId,
    /// The materialised temporal-entity document.
    TemporalData,
}

#[cfg(test)]
mod tests {
    use crate::snapshot::{SnapshotEntities, SnapshotTemporalEntities, Snapshots};
    use sea_query::Iden;

    #[test]
    fn snapshot_relation_names() {
        assert_eq!(Snapshots::Table.to_string(), "snapshots");
        assert_eq!(Snapshots::SnapshotKey.to_string(), "snapshot_key");
        assert_eq!(Snapshots::LastUsedAt.to_string(), "last_used_at");
        assert_eq!(SnapshotEntities::Table.to_string(), "snapshot_entities");
        assert_eq!(SnapshotEntities::SnapshotKey.to_string(), "snapshot_key");
        assert_eq!(SnapshotTemporalEntities::SnapshotKey.to_string(), "snapshot_key");
        assert_eq!(SnapshotEntities::EntityData.to_string(), "entity_data");
        assert_eq!(SnapshotTemporalEntities::Table.to_string(), "snapshot_temporal_entities");
        assert_eq!(SnapshotTemporalEntities::TemporalData.to_string(), "temporal_data");
    }
}
