//! When one write's idempotency record may be removed.
//!
//! A `write_records` row persists the instant a replayed write must reuse, so
//! that the replay collides with the rows it would otherwise duplicate on
//! `UNIQUE (recorded_at, write_id, write_ordinal)`. It may therefore only go
//! once nothing it protects is left: no attribute history row, no scope history
//! row, and no queued append intent that could still be delivered under its
//! identity. Both callers that release records — Temporal Evolution deletion
//! (clause 5.6.16) and the operator retention sweep — apply that one rule, so
//! it is written once here.

use crate::error::TemporalStoreError;
use andromeda_postgresql_schema::{
    queue::{TemporalAppendIntents, WriteRecords},
    temporal::{AttributeHistory, ScopeHistory},
};
use andromeda_postgresql_seaquery::{cast::cast, pg_array::any, pg_type::PostgresType};
use chrono::{DateTime, Utc};
use sea_query::{Cond, DeleteStatement, Expr, ExprTrait, Iden, IntoIden, IntoTableRef, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, Postgres, Transaction, postgres::PgPool};
use uuid::Uuid;

/// The `attribute_history` alias inside a protection subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "ah"]
struct Ah;

/// The `scope_history` alias inside a protection subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "sh"]
struct Sh;

/// The `temporal_append_intents` alias inside a protection subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "ti"]
struct Ti;

/// Release the named idempotency records that no longer protect an effect.
///
/// Runs inside the caller's transaction so that the records go exactly when the
/// effects they protected do.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when the statement fails.
pub(crate) async fn release_named(tx: &mut Transaction<'_, Postgres>, write_ids: &[Uuid]) -> Result<u64, TemporalStoreError> {
    let mut statement = base_release();
    statement.and_where(Expr::col(WriteRecords::WriteId).eq(any(cast(Expr::val(write_ids.to_vec()), PostgresType::UuidArray))));
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_with(AssertSqlSafe(sql), values).execute(&mut **tx).await?.rows_affected())
}

/// Release every unprotected idempotency record recorded before a cutoff.
///
/// The cutoff is what keeps the sweep off records of writes that are still in
/// flight: a record younger than the horizon is left alone whether or not it
/// currently protects anything.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when the statement fails.
pub(crate) async fn release_behind_horizon(pool: &PgPool, cutoff: DateTime<Utc>) -> Result<u64, TemporalStoreError> {
    let mut statement = base_release();
    statement.and_where(Expr::col(WriteRecords::RecordedAt).lt(cutoff));
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_with(AssertSqlSafe(sql), values).execute(pool).await?.rows_affected())
}

/// The delete of `write_records` guarded by the three effect protections, before
/// the caller adds the predicate that selects which records to consider.
fn base_release() -> DeleteStatement {
    Query::delete().from_table(WriteRecords::Table).cond_where(unprotected()).to_owned()
}

/// The three effects a write record outlives nothing without: no attribute
/// history row, no scope history row and no queued append intent references it.
///
/// A history protection correlates on the write's instant as well as its
/// identity. Every history row of a write carries the `recorded_at` of its
/// `write_records` row -- the committer mints one record per commit, every
/// intent of that commit is refused unless it carries it, and neither replay
/// nor instance modification rewrites it -- so the extra equality selects no
/// fewer rows. What it buys is the access path: the only index on either
/// hypertable that carries `write_id` is `uq_*_write (recorded_at, write_id,
/// write_ordinal)`, which a probe on `write_id` alone cannot seek into, so every
/// clause 5.6.16 deletion and every retention sweep probed every chunk. Leading
/// with `recorded_at` seeks that index and lets the planner exclude every other
/// chunk. An append intent's protection needs no instant:
/// `uq_temporal_append_intents_write_identity` already leads with `write_id`.
fn unprotected() -> Cond {
    Cond::all()
        .add(no_history_reference(
            AttributeHistory::Table,
            AttributeHistory::WriteId,
            AttributeHistory::RecordedAt,
            Ah,
        ))
        .add(no_history_reference(ScopeHistory::Table, ScopeHistory::WriteId, ScopeHistory::RecordedAt, Sh))
        .add(no_intent_reference(TemporalAppendIntents::Table, TemporalAppendIntents::WriteId, Ti))
}

/// `NOT EXISTS` protection correlating a history row's write instant and
/// identity to the canonical `write_records.recorded_at` and `write_id`.
fn no_history_reference(
    table: impl IntoTableRef,
    write_id: impl IntoIden + Copy + 'static,
    recorded_at: impl IntoIden + Copy + 'static,
    alias: impl IntoIden + Copy + 'static,
) -> SimpleExpr {
    let subquery = Query::select()
        .expr(Expr::val(1))
        .from_as(table, alias)
        .and_where(Expr::col((alias, recorded_at)).equals((WriteRecords::Table, WriteRecords::RecordedAt)))
        .and_where(Expr::col((alias, write_id)).equals((WriteRecords::Table, WriteRecords::WriteId)))
        .to_owned();
    Expr::exists(subquery).not()
}

/// `NOT EXISTS` protection correlating a queued intent's write identity to the
/// canonical `write_records.write_id`.
fn no_intent_reference(table: impl IntoTableRef, write_id: impl IntoIden + Copy + 'static, alias: impl IntoIden + Copy + 'static) -> SimpleExpr {
    let subquery = Query::select()
        .expr(Expr::val(1))
        .from_as(table, alias)
        .and_where(Expr::col((alias, write_id)).equals((WriteRecords::Table, WriteRecords::WriteId)))
        .to_owned();
    Expr::exists(subquery).not()
}

#[cfg(test)]
mod tests {
    use crate::write_record_release::{Ah, Sh, Ti, base_release, no_history_reference, no_intent_reference};
    use andromeda_postgresql_schema::{
        queue::TemporalAppendIntents,
        temporal::{AttributeHistory, ScopeHistory},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};

    fn render(protection: SimpleExpr) -> String {
        Query::select().expr(protection).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_history_protection_correlates_on_the_write_instant_and_identity() {
        let attribute = render(no_history_reference(
            AttributeHistory::Table,
            AttributeHistory::WriteId,
            AttributeHistory::RecordedAt,
            Ah,
        ));
        assert!(
            attribute.contains(
                r#"NOT EXISTS(SELECT 1 FROM "attribute_history" AS "ah" WHERE "ah"."recorded_at" = "write_records"."recorded_at" AND "ah"."write_id" = "write_records"."write_id")"#
            ),
            "{attribute}"
        );
        let scope = render(no_history_reference(ScopeHistory::Table, ScopeHistory::WriteId, ScopeHistory::RecordedAt, Sh));
        assert!(
            scope.contains(r#""sh"."recorded_at" = "write_records"."recorded_at" AND "sh"."write_id" = "write_records"."write_id""#),
            "{scope}"
        );
    }

    #[test]
    fn an_intent_protection_correlates_on_the_write_identity_alone() {
        let sql = render(no_intent_reference(TemporalAppendIntents::Table, TemporalAppendIntents::WriteId, Ti));
        assert!(
            sql.contains(r#"NOT EXISTS(SELECT 1 FROM "temporal_append_intents" AS "ti" WHERE "ti"."write_id" = "write_records"."write_id")"#),
            "{sql}"
        );
        assert!(!sql.contains("recorded_at"), "{sql}");
    }

    #[test]
    fn every_release_applies_the_three_protections() {
        let sql = base_release().to_string(PostgresQueryBuilder);
        assert_eq!(sql.matches("NOT EXISTS").count(), 3, "{sql}");
        for relation in ["attribute_history", "scope_history", "temporal_append_intents"] {
            assert!(sql.contains(relation), "missing {relation}: {sql}");
        }
        assert!(sql.contains(r#""ti"."write_id" = "write_records"."write_id""#), "{sql}");
        assert!(sql.contains(r#""ah"."recorded_at" = "write_records"."recorded_at""#), "{sql}");
        assert!(sql.contains(r#""sh"."recorded_at" = "write_records"."recorded_at""#), "{sql}");
    }
}
