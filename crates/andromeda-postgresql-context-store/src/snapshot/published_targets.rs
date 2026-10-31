//! Listing the published branch of every Snapshot a tenant holds.
//!
//! A tenant runtime matches entity changes against the Subscriptions cached
//! per context target, and Subscriptions created under `NGSILD-Snapshot` live
//! in the Snapshot's own branch. A runtime built after the tenant was evicted
//! therefore loads every published branch beside the live one, which needs the
//! targets those branches are addressed by.

use crate::store_fault::{persistence_error, undecodable};
use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::snapshot::Snapshots;
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_snapshot::error::SnapshotError;
use sea_query::{Iden, Query};
use sqlx::{PgPool, Row, postgres::PgRow};

/// The published branch of every Snapshot of the tenant `pool` serves.
pub(crate) async fn read_published_targets(pool: &PgPool) -> Result<Vec<SnapshotTarget>, SnapshotError> {
    let rows = Query::select()
        .columns([Snapshots::SnapshotKey, Snapshots::Id])
        .from(Snapshots::Table)
        .to_owned()
        .to_query()
        .fetch_all(pool)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    rows.iter().map(row_to_published_target).collect()
}

/// The published branch one control record names.
fn row_to_published_target(row: &PgRow) -> Result<SnapshotTarget, SnapshotError> {
    let raw_key: i64 = row
        .try_get(Snapshots::SnapshotKey.unquoted())
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    let key = SnapshotKey::try_from(raw_key).map_err(|error| SnapshotError::from(undecodable(&error)))?;
    let id: String = row
        .try_get(Snapshots::Id.unquoted())
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    let id = id.parse::<Identifier>().map_err(|error| SnapshotError::from(undecodable(&error)))?;
    Ok(SnapshotTarget::published(key, id))
}
