//! A repack's setup: the log table and the trigger that fills it.
//!
//! The setup runs in one transaction under the table's `ACCESS EXCLUSIVE`
//! lock, taken in bounded attempts, so no write is in flight while the
//! trigger is installed and every later write is logged. The extension's
//! statements are read under the same lock, so the indexes they redefine
//! are the indexes the table has. The run's row is written before the setup
//! commits: a broker that stops after the commit leaves a row that names
//! the objects to clean up.

use crate::{
    error::MaintenanceError,
    repack::{
        RepackAbandonment,
        RepackSubject,
        guard_session::{self, SetupEnd},
        lock_acquisition::{self, LockOutcome},
        table_script::{self, RepackTableScript},
    },
    state::repack_run,
};
use andromeda_postgresql_seaquery::{
    command::Command,
    relation_lock::LockMode,
    repack::{conflicted_triggers, disable_autovacuum, invalid_index_present},
    statement::Statement,
    transaction::{Isolation, TransactionStart},
};
use sea_query::TableName;
use sqlx::PgConnection;
use std::slice;

/// How a setup ended.
pub(crate) enum SetupOutcome {
    /// The objects are committed and the guard holds its lock.
    Ready(Box<RepackTableScript>),
    /// The setup rolled back and left nothing behind.
    Abandoned(RepackAbandonment),
}

/// Install the log table and the trigger on the subject's table, and commit
/// them once the guard session `guard_pid` is queued for its lock on
/// `guard`.
pub(crate) async fn set_up(
    platform: &mut PgConnection,
    main: &mut PgConnection,
    guard: &mut PgConnection,
    guard_pid: i32,
    subject: RepackSubject<'_>,
    table: &TableName,
) -> Result<SetupOutcome, MaintenanceError> {
    let relid = subject.relid;
    let mut setup = TransactionStart::at(Isolation::ReadCommitted).begin_on(main).await?;
    if lock_acquisition::acquire(&mut setup, slice::from_ref(table), LockMode::AccessExclusive, None).await? == LockOutcome::Abandoned {
        setup.rollback().await?;
        return Ok(SetupOutcome::Abandoned(RepackAbandonment::LockWait));
    }
    if invalid_index_present(relid).to_query_scalar::<bool>().fetch_one(&mut *setup).await? {
        setup.rollback().await?;
        return Ok(SetupOutcome::Abandoned(RepackAbandonment::InvalidIndex));
    }
    if conflicted_triggers(relid).to_query_scalar::<bool>().fetch_one(&mut *setup).await? {
        setup.rollback().await?;
        return Ok(SetupOutcome::Abandoned(RepackAbandonment::ConflictingTrigger));
    }
    let script = match table_script::read_table_script(&mut setup, subject.row.branch(), subject.heap, relid).await? {
        Ok(script) => script,
        Err(absence) => {
            setup.rollback().await?;
            return Ok(SetupOutcome::Abandoned(RepackAbandonment::ScriptAbsent(absence)));
        }
    };
    for command in [&script.create_pktype, &script.create_log, &script.create_trigger, &script.enable_trigger] {
        command.to_query().execute(&mut *setup).await?;
    }
    disable_autovacuum(&format!("repack.log_{relid}")).to_query().execute(&mut *setup).await?;
    repack_run::record_start(platform, subject.tenant, subject.row, relid).await?;
    match guard_session::commit_setup_behind_guard(setup, guard, guard_pid, table, relid).await? {
        SetupEnd::Committed => Ok(SetupOutcome::Ready(Box::new(script))),
        SetupEnd::Yielded => {
            repack_run::record_end(platform, subject.tenant.key, subject.row).await?;
            Ok(SetupOutcome::Abandoned(RepackAbandonment::Yielded))
        }
    }
}
