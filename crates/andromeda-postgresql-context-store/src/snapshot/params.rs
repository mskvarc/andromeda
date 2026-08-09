//! Statement-shaped parameters internal to the snapshot repository.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
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
    pub snapshot_id: &'a Uri,
    /// Optional entity type filter.
    pub entity_type: Option<&'a TypeExprRaw>,
    /// Optional set of entity identifiers to filter by.
    pub entity_ids: Option<&'a [Uri]>,
    /// Maximum number of rows to return.
    pub limit: i64,
    /// Offset into the result set.
    pub offset: i64,
}
