//! The objects an interrupted repack leaves, and their cleanup.
//!
//! A run that stops after its setup committed leaves its log table, its copy
//! table and the trigger that logs every write of the table, which keeps
//! doubling the table's writes until the objects are dropped. They are found
//! from the catalog alone (`repack.log_<oid>`, `repack.table_<oid>`, a
//! `repack_trigger` on the table), so a run row lost with its platform
//! database, or a run of the command line, is found too. A table's objects
//! are cleaned up only while nobody holds its lock, under `ACCESS EXCLUSIVE`
//! taken in bounded attempts, by the extension's own cleanup function.

use crate::{
    branch::{Branch, BranchTarget},
    candidate::{MaintenanceTarget, heap_target::HeapTarget},
    catalog_counter::{CatalogCounter, object_id},
    error::MaintenanceError,
    repack::{
        lock_acquisition::{self, LockOutcome},
        table_lock::{RepackTableLock, TableLockTaken},
    },
};
use andromeda_postgresql_seaquery::{
    relation_lock::LockMode,
    repack::drop_objects,
    repack_catalog::{relation_identity, repack_schema_tables, triggered_tables},
    statement::Statement,
    transaction::{Isolation, TransactionStart},
};
use sea_query::{Alias, TableName};
use sqlx::{PgConnection, Row};
use std::str::FromStr;

/// One object a run creates.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum LeftoverObject {
    /// `repack.log_<oid>`, which the trigger writes to.
    LogTable,
    /// `repack.table_<oid>`, the copy, or after a swap the old files.
    CopyTable,
    /// The `repack_trigger` on the table.
    Trigger,
}

/// The objects a run left for one table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RepackLeftover {
    /// The table's OID.
    pub(crate) relid: u32,
    /// What is left, in [`LeftoverObject`] order.
    pub(crate) objects: Vec<LeftoverObject>,
}

/// How one cleanup ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CleanupOutcome {
    /// Every object of the table is dropped.
    Cleaned,
    /// Another session holds the table's lock: a run is still going.
    HeldElsewhere,
    /// The table's `ACCESS EXCLUSIVE` lock was not granted in time.
    LockWait,
    /// No table has the OID any more.
    Vanished,
}

/// The leftovers the names of the `repack` schema's tables and the OIDs of
/// the tables carrying a trigger make, one per table, in OID order. A table
/// of the schema named otherwise is not a run's and is ignored.
pub(crate) fn leftovers(tables: &[String], triggered: &[u32]) -> Vec<RepackLeftover> {
    let named = tables.iter().filter_map(|name| {
        let object = |prefix: &str, object: LeftoverObject| name.strip_prefix(prefix).and_then(|oid| u32::from_str(oid).ok()).map(|oid| (oid, object));
        object("log_", LeftoverObject::LogTable).or_else(|| object("table_", LeftoverObject::CopyTable))
    });
    let mut objects = named.chain(triggered.iter().map(|&relid| (relid, LeftoverObject::Trigger))).collect::<Vec<_>>();
    objects.sort_unstable();
    objects.dedup();
    objects
        .chunk_by(|left, right| left.0 == right.0)
        .filter_map(|table| {
            table.first().map(|&(relid, _)| RepackLeftover {
                relid,
                objects: table.iter().map(|&(_, object)| object).collect(),
            })
        })
        .collect()
}

/// Every leftover in the tenant database.
pub(crate) async fn read_leftovers(connection: &mut PgConnection) -> Result<Vec<RepackLeftover>, MaintenanceError> {
    let tables = repack_schema_tables().to_query_scalar::<String>().fetch_all(&mut *connection).await?;
    let triggered = triggered_tables()
        .to_query_scalar::<i64>()
        .fetch_all(connection)
        .await?
        .into_iter()
        .map(|relid| object_id(relid, CatalogCounter::RelationOid))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(leftovers(&tables, &triggered))
}

/// The schema and name of the table `relid`, `None` when none has the OID.
pub(crate) async fn identity(connection: &mut PgConnection, relid: u32) -> Result<Option<(String, String)>, MaintenanceError> {
    let row = relation_identity(relid).to_query().fetch_optional(connection).await?;
    Ok(row
        .map(|row| {
            row.try_get::<String, _>(0)
                .and_then(|schema| row.try_get::<String, _>(1).map(|name| (schema, name)))
        })
        .transpose()?)
}

/// The maintenance target the table named `schema.name` is, `None` for a
/// table outside the allowlist or a schema that is no maintenance branch.
pub(crate) fn repack_target(schema: &str, name: &str) -> Option<BranchTarget> {
    let branch = Branch::from_schema(schema)?;
    let heap = HeapTarget::from_str(name).ok()?;
    BranchTarget::new(branch, MaintenanceTarget::Repack(heap)).ok()
}

/// Drop every object a run left for the table `relid`, unless a run still
/// holds the table's lock.
pub(crate) async fn clean_up(connection: &mut PgConnection, relid: u32) -> Result<CleanupOutcome, MaintenanceError> {
    let Some((schema, name)) = identity(&mut *connection, relid).await? else {
        return Ok(CleanupOutcome::Vanished);
    };
    let lock = RepackTableLock::of(relid);
    if lock.try_take(&mut *connection).await? == TableLockTaken::HeldElsewhere {
        return Ok(CleanupOutcome::HeldElsewhere);
    }
    let outcome = drop_under_lock(&mut *connection, relid, TableName::from((Alias::new(schema), Alias::new(name)))).await;
    lock.release(connection).await?;
    outcome
}

/// Drop the objects of the table `relid`, named `table`, under its
/// `ACCESS EXCLUSIVE` lock.
async fn drop_under_lock(connection: &mut PgConnection, relid: u32, table: TableName) -> Result<CleanupOutcome, MaintenanceError> {
    let mut transaction = TransactionStart::at(Isolation::ReadCommitted).begin_on(connection).await?;
    match lock_acquisition::acquire(&mut transaction, &[table], LockMode::AccessExclusive, None).await? {
        LockOutcome::Acquired => {}
        LockOutcome::Abandoned => {
            transaction.rollback().await?;
            return Ok(CleanupOutcome::LockWait);
        }
    }
    drop_objects(relid).to_query().execute(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(CleanupOutcome::Cleaned)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        branch::{Branch, BranchTarget},
        candidate::{MaintenanceTarget, heap_target::HeapTarget},
        repack::leftover::{LeftoverObject, RepackLeftover, leftovers, repack_target},
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_tables_log_copy_and_trigger_make_one_leftover() {
        let tables = ["log_16384".to_owned(), "table_16384".to_owned()];
        assert_eq!(
            leftovers(&tables, &[16_384]),
            [RepackLeftover {
                relid: 16_384,
                objects: vec![LeftoverObject::LogTable, LeftoverObject::CopyTable, LeftoverObject::Trigger],
            }]
        );
    }

    #[test]
    fn a_table_of_the_repack_schema_that_is_no_runs_is_ignored() {
        let tables = ["operator_notes".to_owned(), "log_x".to_owned(), "log_16390".to_owned(), "tables".to_owned()];
        assert_eq!(
            leftovers(&tables, &[]),
            [RepackLeftover {
                relid: 16_390,
                objects: vec![LeftoverObject::LogTable],
            }]
        );
    }

    #[test]
    fn each_table_gets_its_own_leftover_in_oid_order() {
        let tables = ["log_20000".to_owned(), "log_16384".to_owned()];
        assert_eq!(
            leftovers(&tables, &[20_000, 16_384]).iter().map(|leftover| leftover.relid).collect::<Vec<_>>(),
            [16_384, 20_000]
        );
    }

    #[test]
    fn only_an_allowlisted_heap_of_a_branch_is_a_repack_target() {
        let snapshot = Branch::Snapshot(SnapshotKey::try_from(7).expect("positive snapshot key"));
        assert_eq!(
            repack_target("snap_7", "notification_deliveries"),
            BranchTarget::new(snapshot, MaintenanceTarget::Repack(HeapTarget::NotificationDeliveries)).ok()
        );
        assert_eq!(repack_target("public", "attribute_history"), None);
        assert_eq!(repack_target("repack", "notification_deliveries"), None);
        assert_eq!(repack_target("snap_7", "pending_side_effects"), None);
    }
}
