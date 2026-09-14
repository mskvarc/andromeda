//! Enumerating the context branches a per-branch delivery drain sweeps.
//!
//! Deliveries live in the queue of the branch whose subscription matched the
//! change: `public` for the live branch and `snap_<snapshot_key>` for each
//! Snapshot branch. A drain therefore visits the live branch and every Snapshot
//! branch in turn; a branch reaped between this enumeration and the claim shows
//! up as a missing schema, which the caller skips rather than fails on.

use crate::error::ContextStoreError;
use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
use andromeda_ngsi_ld_lexicon::uri::UriParseError;
use andromeda_postgresql_schema::snapshot::Snapshots;
use sea_query::{PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Row, postgres::PgPool};

/// The branches a per-branch drain sweeps: the live branch, then every Snapshot
/// branch published under `snap_<snapshot_key>`.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the Snapshot enumeration fails,
/// or [`ContextStoreError::Internal`] when a stored Snapshot key or URI cannot
/// be read back as its domain type.
pub(super) async fn drainable_targets(pool: &PgPool) -> Result<Vec<ContextTarget>, ContextStoreError> {
    let statement = Query::select()
        .columns([Snapshots::SnapshotKey, Snapshots::Id])
        .from(Snapshots::Table)
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(pool).await?;

    let mut targets = Vec::with_capacity(rows.len() + 1);
    targets.push(ContextTarget::Live);
    for row in rows {
        let raw_key: i64 = row.try_get("snapshot_key")?;
        let raw_id: String = row.try_get("id")?;
        let key = SnapshotKey::try_from(raw_key).map_err(|error| ContextStoreError::Internal(format!("snapshot branch key: {error}")))?;
        let id = raw_id
            .parse()
            .map_err(|error: UriParseError| ContextStoreError::Internal(format!("snapshot branch uri: {error}")))?;
        targets.push(ContextTarget::Snapshot(SnapshotTarget::published(key, id)));
    }
    Ok(targets)
}

/// Whether a persistence failure is a Snapshot branch reaped out from under the
/// drain: its schema or a relation in it no longer exists.
///
/// PostgreSQL raises `3F000` (`invalid_schema_name`) or `42P01`
/// (`undefined_table`); either means the branch was dropped between the
/// enumeration and the claim, so the drain moves on rather than failing.
#[must_use]
pub(super) fn is_reaped_branch(error: &ContextStoreError) -> bool {
    let ContextStoreError::Database(sqlx::Error::Database(database_error)) = error else {
        return false;
    };
    matches!(database_error.code().as_deref(), Some("3F000" | "42P01"))
}
