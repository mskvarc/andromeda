//! The database session a clone's copy runs in, recorded so that deleting the
//! clone or its source can end the copy instead of waiting for it.
//!
//! The copy holds the control records of the clone and of its source
//! `FOR KEY SHARE` for as long as the source is large, and a deletion takes
//! `FOR UPDATE` on its control record before it drops a branch schema. A
//! deletion therefore first ends every copy into or out of the Snapshot it
//! deletes. The session is terminated, not cancelled: a cancellation reaches
//! only a statement in progress, and a copy between two statements would carry
//! on holding its locks.
//!
//! The copy records its session before it takes either lock, from a second
//! connection, so the record is visible to a deletion while the copy runs. A
//! deletion that locks first makes the record's foreign key check wait for it
//! and then fail, and the copy never starts. A pooled session outlives the copy,
//! so the record names the copy's transaction by its start as well as the
//! process: a record left by an ended copy matches no running transaction.

use crate::store_fault::persistence_error;
use andromeda_context_space::snapshot_key::SnapshotKey;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::snapshot::{SnapshotCloneCopies, Snapshots};
use andromeda_postgresql_seaquery::{
    catalog_function::{backend_pid, terminate_backend},
    clock::now,
    statement::Statement,
    system_catalog::PgStatActivity,
};
use andromeda_snapshot::error::SnapshotError;
use chrono::{DateTime, Utc};
use sea_query::{DeleteStatement, Expr, ExprTrait, InsertStatement, JoinType, OnConflict, Query, SelectStatement};
use sqlx::{PgPool, Postgres, Transaction};

/// The session a clone's copy runs in, named by its backend process and the
/// start of the copy's transaction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CopySession {
    /// The process id of the session's backend, an `integer` as
    /// `pg_backend_pid()` and `pg_stat_activity.pid` give it.
    backend_pid: i32,
    /// The instant the copy's transaction began.
    transaction_started_at: DateTime<Utc>,
}

/// `SELECT pg_backend_pid(), NOW()`: the session running the
/// statement, and the start of its transaction.
fn own_session_statement() -> SelectStatement {
    Query::select().expr(backend_pid()).expr(now()).to_owned()
}

/// Read the session `transaction` runs in.
///
/// # Errors
///
/// Returns [`SnapshotError`] when the session cannot be read.
pub(crate) async fn own_copy_session(transaction: &mut Transaction<'_, Postgres>) -> Result<CopySession, SnapshotError> {
    let (backend_pid, transaction_started_at) = own_session_statement()
        .to_query_as::<(i32, DateTime<Utc>)>()
        .fetch_one(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(CopySession {
        backend_pid,
        transaction_started_at,
    })
}

/// Record `session` as the copy into the clone `clone_id`, replacing the record
/// of an earlier attempt; records nothing for a clone that is gone.
fn record_statement(clone_id: &Identifier, session: CopySession) -> Result<InsertStatement, SnapshotError> {
    let clone = Query::select()
        .column(Snapshots::SnapshotKey)
        .expr(Expr::val(session.backend_pid))
        .expr(Expr::val(session.transaction_started_at))
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::Id).eq(clone_id.as_str()))
        .to_owned();
    let mut statement = Query::insert();
    statement
        .into_table(SnapshotCloneCopies::Table)
        .columns([
            SnapshotCloneCopies::SnapshotKey,
            SnapshotCloneCopies::BackendPid,
            SnapshotCloneCopies::TransactionStartedAt,
        ])
        .select_from(clone)
        .map_err(|error| persistence_error::<SnapshotError>(error))?
        .on_conflict(
            OnConflict::column(SnapshotCloneCopies::SnapshotKey)
                .update_columns([SnapshotCloneCopies::BackendPid, SnapshotCloneCopies::TransactionStartedAt])
                .to_owned(),
        );
    Ok(statement)
}

/// Record, and commit at once, `session` as the copy into the clone `clone_id`.
///
/// # Errors
///
/// Returns [`SnapshotError`] when the record cannot be written.
pub(crate) async fn record_copy_session(pool: &PgPool, clone_id: &Identifier, session: CopySession) -> Result<(), SnapshotError> {
    record_statement(clone_id, session)?
        .to_query()
        .execute(pool)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(())
}

/// Remove the copy record of the Snapshot `snapshot_key`.
fn forget_statement(snapshot_key: SnapshotKey) -> DeleteStatement {
    Query::delete()
        .from_table(SnapshotCloneCopies::Table)
        .and_where(Expr::col(SnapshotCloneCopies::SnapshotKey).eq(snapshot_key.as_i64()))
        .to_owned()
}

/// Remove the copy record of the Snapshot `snapshot_key`, which a capture never
/// had.
///
/// # Errors
///
/// Returns [`SnapshotError`] when the record cannot be removed.
pub(crate) async fn forget_copy_session(transaction: &mut Transaction<'_, Postgres>, snapshot_key: SnapshotKey) -> Result<(), SnapshotError> {
    forget_statement(snapshot_key)
        .to_query()
        .execute(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(())
}

/// `SELECT pg_terminate_backend(pid)` of every session still running the copy
/// into the Snapshot `id` or into a clone of it.
fn end_statement(id: &Identifier) -> SelectStatement {
    Query::select()
        .expr(terminate_backend(Expr::col((PgStatActivity::Table, PgStatActivity::Pid))))
        .from(SnapshotCloneCopies::Table)
        .join(
            JoinType::Join,
            Snapshots::Table,
            Expr::col((Snapshots::Table, Snapshots::SnapshotKey)).equals((SnapshotCloneCopies::Table, SnapshotCloneCopies::SnapshotKey)),
        )
        .join(
            JoinType::Join,
            PgStatActivity::Table,
            Expr::col((PgStatActivity::Table, PgStatActivity::Pid))
                .equals((SnapshotCloneCopies::Table, SnapshotCloneCopies::BackendPid))
                .and(
                    Expr::col((PgStatActivity::Table, PgStatActivity::XactStart))
                        .equals((SnapshotCloneCopies::Table, SnapshotCloneCopies::TransactionStartedAt)),
                ),
        )
        .and_where(
            Expr::col((Snapshots::Table, Snapshots::Id))
                .eq(id.as_str())
                .or(Expr::col((Snapshots::Table, Snapshots::CloneSourceId)).eq(id.as_str())),
        )
        .to_owned()
}

/// End every session still copying into the Snapshot `id` or out of it into a
/// clone, so the deletion's lock on its control record is granted once those
/// sessions close.
///
/// # Errors
///
/// Returns [`SnapshotError`] when the sessions cannot be read or signalled.
pub(crate) async fn end_copy_sessions(transaction: &mut Transaction<'_, Postgres>, id: &Identifier) -> Result<(), SnapshotError> {
    end_statement(id)
        .to_query()
        .execute(&mut **transaction)
        .await
        .map_err(|error| persistence_error::<SnapshotError>(error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::snapshot::clone_copy_session::{CopySession, end_statement, forget_statement, own_session_statement, record_statement};
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn session() -> CopySession {
        CopySession {
            backend_pid: 4242,
            transaction_started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().expect("valid instant"),
        }
    }

    #[test]
    fn the_copy_reads_its_own_backend_and_the_start_of_its_transaction() {
        assert_eq!(own_session_statement().to_string(PostgresQueryBuilder), "SELECT pg_backend_pid(), NOW()");
    }

    #[test]
    fn the_copy_session_is_recorded_against_the_clone_and_replaces_an_earlier_attempt() {
        let clone = "urn:ngsi-ld:Snapshot:clone".parse().expect("valid snapshot uri");
        let sql = record_statement(&clone, session()).expect("a recording").to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"INSERT INTO "snapshot_clone_copies" ("snapshot_key", "backend_pid", "transaction_started_at") "#,
                r#"SELECT "snapshot_key", 4242, '2026-01-01 00:00:00.000000 +00:00' FROM "snapshots" WHERE "id" = 'urn:ngsi-ld:Snapshot:clone' "#,
                r#"ON CONFLICT ("snapshot_key") DO UPDATE SET "backend_pid" = "excluded"."backend_pid", "transaction_started_at" = "excluded"."transaction_started_at""#,
            )
        );
    }

    #[test]
    fn the_publication_forgets_the_copy_session_of_its_snapshot() {
        let sql = forget_statement(SnapshotKey::try_from(7).expect("positive key")).to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"DELETE FROM "snapshot_clone_copies" WHERE "snapshot_key" = 7"#);
    }

    /// The transaction start keeps a pooled session that has moved on from the
    /// copy out of reach.
    #[test]
    fn a_deletion_terminates_the_copies_into_and_out_of_its_snapshot_still_in_their_transaction() {
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        let sql = end_statement(&id).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT pg_terminate_backend("pg_stat_activity"."pid") FROM "snapshot_clone_copies" "#,
                r#"JOIN "snapshots" ON "snapshots"."snapshot_key" = "snapshot_clone_copies"."snapshot_key" "#,
                r#"JOIN "pg_stat_activity" ON "pg_stat_activity"."pid" = "snapshot_clone_copies"."backend_pid" "#,
                r#"AND "pg_stat_activity"."xact_start" = "snapshot_clone_copies"."transaction_started_at" "#,
                r#"WHERE "snapshots"."id" = 'urn:ngsi-ld:Snapshot:s' OR "snapshots"."clone_source_id" = 'urn:ngsi-ld:Snapshot:s'"#,
            )
        );
    }
}
