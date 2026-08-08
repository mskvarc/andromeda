//! Statement-shaped parameters internal to the snapshot repository.

use andromeda_ngsi_ld_lexicon::{ngsi_ld_urn::SnapshotId, uri::Uri};
use sqlx::postgres::PgPool;

/// Parameters for a paginated snapshot table query.
pub struct SnapshotQueryParams<'a> {
    /// Connection pool to execute the query against.
    pub pool: &'a PgPool,
    /// Name of the snapshot table to query.
    pub table: &'a str,
    /// Name of the data column to project.
    pub data_column: &'a str,
    /// Snapshot identifier to filter by.
    pub snapshot_id: &'a SnapshotId,
    /// Optional entity type filter.
    pub entity_type: Option<&'a str>,
    /// Optional set of entity identifiers to filter by.
    pub entity_ids: Option<&'a [Uri]>,
    /// Maximum number of rows to return.
    pub limit: i64,
    /// Offset into the result set.
    pub offset: i64,
}
