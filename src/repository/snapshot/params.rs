//! Statement-shaped parameters internal to the snapshot repository.

use andromeda_ngsi_ld_lexicon::{ngsi_ld_urn::SnapshotId, uri::Uri};
use sqlx::postgres::PgPool;

/// Parameters for a paginated snapshot table query.
pub(crate) struct SnapshotQueryParams<'a> {
    pub pool: &'a PgPool,
    pub table: &'a str,
    pub data_column: &'a str,
    pub snapshot_id: &'a SnapshotId,
    pub entity_type: Option<&'a str>,
    pub entity_ids: Option<&'a [Uri]>,
    pub limit: i64,
    pub offset: i64,
}
