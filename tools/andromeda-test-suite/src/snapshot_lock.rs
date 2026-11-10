//! The locks a Snapshot's lifecycle takes on its rows in the tenant database, as a [`crate::row_hold::RowHold`] takes
//! them to stand for a transition still running.

use andromeda_postgresql_schema::snapshot::{SnapshotPreparation, Snapshots};
use ngsi_ld::snapshot::SnapshotId;
use sea_query::{Expr, ExprTrait, JoinType, LockType, Query, SelectStatement};

/// The lock a Snapshot's publication holds while it analyses the branch it prepared, renames it into place and
/// records the Snapshot's terminal status: its control record and its preparation row, `FOR UPDATE`.
#[must_use]
pub fn publication_lock(id: &SnapshotId) -> SelectStatement {
    Query::select()
        .column((Snapshots::Table, Snapshots::SnapshotKey))
        .from(Snapshots::Table)
        .join(
            JoinType::Join,
            SnapshotPreparation::Table,
            Expr::col((SnapshotPreparation::Table, SnapshotPreparation::SnapshotKey)).equals((Snapshots::Table, Snapshots::SnapshotKey)),
        )
        .and_where(Expr::col((Snapshots::Table, Snapshots::Id)).eq(id.as_str()))
        .lock(LockType::Update)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::snapshot_lock::publication_lock;
    use googletest::prelude::*;
    use ngsi_ld::snapshot::SnapshotId;
    use sea_query::PostgresQueryBuilder;

    /// A lock naming no table covers both rows the join reads, as the publication's does.
    #[gtest]
    fn the_publication_lock_holds_the_control_record_and_the_preparation_row_for_update() -> Result<()> {
        let id = "urn:ngsi-ld:Snapshot:s".parse::<SnapshotId>()?;
        verify_that!(
            publication_lock(&id).to_string(PostgresQueryBuilder),
            eq(concat!(
                r#"SELECT "snapshots"."snapshot_key" FROM "snapshots" "#,
                r#"JOIN "snapshot_preparation" ON "snapshot_preparation"."snapshot_key" = "snapshots"."snapshot_key" "#,
                r#"WHERE "snapshots"."id" = 'urn:ngsi-ld:Snapshot:s' FOR UPDATE"#,
            ))
        )
    }
}
