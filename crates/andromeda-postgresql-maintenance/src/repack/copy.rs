//! A repack's copy of its table and the copy's indexes.
//!
//! The copy runs in one repeatable-read transaction whose first statement
//! reads the transactions open at its snapshot, so the swap can wait them
//! out. It empties the log before it reads the table, so a row is either in
//! the copy or logged after it, never both. The copy's `ACCESS SHARE` lock on
//! the table is taken in bounded attempts: with the guard holding its lock,
//! only a schema change waiting for the table can stand in its way, and the
//! run yields to it. Each index is built in a transaction of its own; both
//! the copy and each build may run for the phase budget.

use crate::{
    error::MaintenanceError,
    repack::{
        RepackAbandonment,
        lock_acquisition::{self, LockOutcome},
        old_transactions::{self, OldTransactions},
        table_script::RepackTableScript,
    },
};
use andromeda_postgresql_seaquery::{
    command::Command,
    relation_lock::LockMode,
    repack::{copy_work_mem, create_table, disable_autovacuum},
    setting::Setting,
    statement::Statement,
    transaction::{Isolation, TransactionStart},
};
use sea_query::TableName;
use sqlx::PgConnection;
use std::{slice, time::Duration};

/// How a copy ended.
pub(crate) enum CopyOutcome {
    /// The copy holds every row visible at its snapshot.
    Copied(OldTransactions),
    /// The run yields to a session waiting for the table.
    Abandoned(RepackAbandonment),
}

/// Copy the table `table` into the copy table, the guard session being
/// `guard_pid`, each statement allowed `budget`.
pub(crate) async fn copy_rows(
    main: &mut PgConnection,
    guard_pid: i32,
    script: &RepackTableScript,
    table: &TableName,
    budget: Duration,
) -> Result<CopyOutcome, MaintenanceError> {
    let mut copy = TransactionStart::at(Isolation::RepeatableRead)
        .with_local(Setting::StatementTimeout, &statement_timeout(budget))
        .begin_on(main)
        .await?;
    let old = old_transactions::capture(&mut copy, guard_pid).await?;
    copy_work_mem().to_query().execute(&mut *copy).await?;
    script.delete_log.to_query().execute(&mut *copy).await?;
    if lock_acquisition::acquire(&mut copy, slice::from_ref(table), LockMode::AccessShare, None).await? == LockOutcome::Abandoned {
        copy.rollback().await?;
        return Ok(CopyOutcome::Abandoned(RepackAbandonment::Yielded));
    }
    create_table(script.relid, script.tablespace.as_str()).to_query().execute(&mut *copy).await?;
    if let Some(storage) = &script.alter_col_storage {
        storage.to_query().execute(&mut *copy).await?;
    }
    script.copy.to_query().execute(&mut *copy).await?;
    if let Some(dropped) = &script.drop_columns {
        dropped.to_query().execute(&mut *copy).await?;
    }
    disable_autovacuum(&format!("repack.table_{}", script.relid))
        .to_query()
        .execute(&mut *copy)
        .await?;
    copy.commit().await?;
    Ok(CopyOutcome::Copied(old))
}

/// Build every index of the table on the copy, each statement allowed
/// `budget`.
pub(crate) async fn build_indexes(main: &mut PgConnection, script: &RepackTableScript, budget: Duration) -> Result<(), MaintenanceError> {
    for index in &script.indexes {
        let mut build = TransactionStart::at(Isolation::ReadCommitted)
            .with_local(Setting::StatementTimeout, &statement_timeout(budget))
            .begin_on(&mut *main)
            .await?;
        index.to_query().execute(&mut *build).await?;
        build.commit().await?;
    }
    Ok(())
}

/// `budget` as a `statement_timeout` value, in whole seconds.
fn statement_timeout(budget: Duration) -> String {
    format!("{}s", budget.as_secs())
}

#[cfg(test)]
mod tests {
    use crate::repack::copy::statement_timeout;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn the_budget_is_set_in_whole_seconds() {
        assert_eq!(statement_timeout(Duration::from_secs(4_096)), "4096s");
    }
}
