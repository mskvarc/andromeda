//! The identity of every Snapshot a tenant holds: the surrogate key its
//! branch schemas are named from, and its URI.

use crate::{error::ContextStoreError, snapshot::stored_snapshot_error::StoredSnapshotError};
use andromeda_context_space::snapshot_key::SnapshotKey;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::snapshot::Snapshots;
use andromeda_postgresql_seaquery::statement::Statement;
use sea_query::{Order, Query, SelectStatement};
use sqlx::PgExecutor;

/// One Snapshot's identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SnapshotIdentity {
    /// The surrogate key the Snapshot's branch schemas are named from.
    pub(crate) key: SnapshotKey,
    /// The Snapshot's URI.
    pub(crate) id: Identifier,
}

/// The identity of every Snapshot, in key order.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the read fails, and
/// [`ContextStoreError::StoredSnapshot`] when a stored key or URI does not
/// read back as its domain type.
pub(crate) async fn read_snapshot_identities<'c>(executor: impl PgExecutor<'c>) -> Result<Vec<SnapshotIdentity>, ContextStoreError> {
    let rows = snapshot_identities_statement().to_query_as::<(i64, String)>().fetch_all(executor).await?;
    rows.into_iter()
        .map(|(raw_key, raw_id)| {
            Ok(SnapshotIdentity {
                key: SnapshotKey::try_from(raw_key).map_err(StoredSnapshotError::Key)?,
                id: raw_id.parse().map_err(StoredSnapshotError::Id)?,
            })
        })
        .collect()
}

/// The surrogate key and URI of every Snapshot, in key order.
fn snapshot_identities_statement() -> SelectStatement {
    Query::select()
        .columns([Snapshots::SnapshotKey, Snapshots::Id])
        .from(Snapshots::Table)
        .order_by(Snapshots::SnapshotKey, Order::Asc)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::snapshot::identities::snapshot_identities_statement;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_snapshot_identities_are_read_in_key_order() {
        assert_eq!(
            snapshot_identities_statement().to_string(PostgresQueryBuilder),
            r#"SELECT "snapshot_key", "id" FROM "snapshots" ORDER BY "snapshot_key" ASC"#
        );
    }
}
