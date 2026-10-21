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
//!
//! A release considers only records whose instant it can prove unprotected
//! without reading a converted chunk: those after the latest converted chunk,
//! where the write-identity index answers the proof, and, for the sweep, those
//! before the earliest chunk, where no attribute history row can exist. A
//! record inside a converted chunk stays until its chunk is dropped; keeping a
//! record whose effects are gone changes nothing a replay does, while proving
//! it there would read every compressed batch of the chunk. `scope_history` is
//! a plain table whose write-identity index answers the proof at any instant,
//! and the Scope event retention keeps however old it is protects its record.

use crate::{error::TemporalStoreError, history_chunk_span::HistoryChunkSpan, statement_timeout_allowance::allow_unbounded_statements};
use andromeda_postgresql_schema::{
    queue::{TemporalAppendIntents, WriteRecords},
    temporal::{AttributeHistory, ScopeHistory},
};
use andromeda_postgresql_seaquery::{cast::cast, pg_array::any, pg_type::PostgresType};
use chrono::{DateTime, Utc};
use sea_query::{Cond, DeleteStatement, Expr, ExprTrait, Iden, IntoColumnRef, IntoIden, IntoTableRef, PostgresQueryBuilder, Query, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, Connection, PgConnection, Postgres, Transaction, postgres::PgPool};
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

/// The write-clock instants one release statement considers, applied both to
/// the records and to the history its protections probe, so the probes reach
/// no chunk outside them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ReleaseWindow {
    /// The first instant considered; `None` is unbounded below.
    from: Option<DateTime<Utc>>,
    /// The first instant past the window; `None` is unbounded above.
    before: Option<DateTime<Utc>>,
}

impl ReleaseWindow {
    /// The window as a condition on the instant `column` holds.
    fn condition(self, column: impl IntoColumnRef + Copy) -> Cond {
        let mut condition = Cond::all();
        if let Some(from) = self.from {
            condition = condition.add(Expr::col(column).gte(from));
        }
        if let Some(before) = self.before {
            condition = condition.add(Expr::col(column).lt(before));
        }
        condition
    }
}

/// The window a named release considers: every instant after the latest
/// converted chunk.
const fn named_window(span: HistoryChunkSpan) -> ReleaseWindow {
    ReleaseWindow {
        from: span.compressed_until,
        before: None,
    }
}

/// The windows a sweep behind `cutoff` considers.
///
/// With nothing converted, every instant before the cutoff. Otherwise the
/// instants before the earliest chunk, which hold no history, and those
/// between the latest converted chunk and the cutoff.
fn sweep_windows(span: HistoryChunkSpan, cutoff: DateTime<Utc>) -> Vec<ReleaseWindow> {
    let Some(compressed_until) = span.compressed_until else {
        return vec![ReleaseWindow {
            from: None,
            before: Some(cutoff),
        }];
    };
    let historyless = span.history_from.map(|history_from| ReleaseWindow {
        from: None,
        before: Some(Ord::min(history_from, cutoff)),
    });
    let after_converted = (compressed_until < cutoff).then_some(ReleaseWindow {
        from: Some(compressed_until),
        before: Some(cutoff),
    });
    historyless.into_iter().chain(after_converted).collect()
}

/// Release the named idempotency records that no longer protect an effect.
///
/// Runs inside the caller's transaction so that the records go exactly when the
/// effects they protected do.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when a statement fails.
pub(crate) async fn release_named(tx: &mut Transaction<'_, Postgres>, write_ids: &[Uuid]) -> Result<u64, TemporalStoreError> {
    if write_ids.is_empty() {
        return Ok(0);
    }
    let span = HistoryChunkSpan::read(tx).await?;
    let mut statement = base_release(named_window(span));
    statement.and_where(Expr::col(WriteRecords::WriteId).eq(any(cast(Expr::val(write_ids.to_vec()), PostgresType::UuidArray))));
    execute(tx, &statement).await
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
    let mut connection = pool.acquire().await?;
    let span = HistoryChunkSpan::read(&mut connection).await?;
    let mut released = 0;
    for window in sweep_windows(span, cutoff) {
        released += execute_unbounded(&mut connection, &base_release(window)).await?;
    }
    Ok(released)
}

/// Run one release statement, reporting how many records it removed.
async fn execute(connection: &mut PgConnection, statement: &DeleteStatement) -> Result<u64, TemporalStoreError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_with(AssertSqlSafe(sql), values).execute(connection).await?.rows_affected())
}

/// Run one sweep release statement in a transaction of its own, with no
/// statement timeout, reporting how many records it removed.
async fn execute_unbounded(connection: &mut PgConnection, statement: &DeleteStatement) -> Result<u64, TemporalStoreError> {
    let mut transaction = connection.begin().await?;
    allow_unbounded_statements(&mut transaction).await?;
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let released = sqlx::query_with(AssertSqlSafe(sql), values).execute(&mut *transaction).await?.rows_affected();
    transaction.commit().await?;
    Ok(released)
}

/// The delete of the `write_records` inside `window` guarded by the three
/// effect protections, before a named release adds the identities it
/// considers.
fn base_release(window: ReleaseWindow) -> DeleteStatement {
    Query::delete()
        .from_table(WriteRecords::Table)
        .cond_where(window.condition(WriteRecords::RecordedAt))
        .cond_where(unprotected(window))
        .to_owned()
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
/// history table that carries `write_id` is `uq_*_write_identity (recorded_at,
/// write_id, write_ordinal)`, which a probe on `write_id` alone cannot seek
/// into. Leading with `recorded_at` seeks that index and lets the planner
/// exclude every other chunk of the attribute history hypertable. An append intent's protection needs no instant:
/// `uq_temporal_append_intents_write_identity` already leads with `write_id`.
fn unprotected(window: ReleaseWindow) -> Cond {
    Cond::all()
        .add(no_history_reference(
            AttributeHistory::Table,
            AttributeHistory::WriteId,
            AttributeHistory::RecordedAt,
            Ah,
            window,
        ))
        .add(no_history_reference(
            ScopeHistory::Table,
            ScopeHistory::WriteId,
            ScopeHistory::RecordedAt,
            Sh,
            window,
        ))
        .add(no_intent_reference(TemporalAppendIntents::Table, TemporalAppendIntents::WriteId, Ti))
}

/// `NOT EXISTS` protection correlating a history row's write instant and
/// identity to the canonical `write_records.recorded_at` and `write_id`.
///
/// The history row's instant is bounded by `window` as well: it adds nothing
/// to the equality, but it lets chunk exclusion drop every chunk outside the
/// window, converted ones included, whatever join the planner chooses.
fn no_history_reference(
    table: impl IntoTableRef,
    write_id: impl IntoIden + Copy + 'static,
    recorded_at: impl IntoIden + Copy + 'static,
    alias: impl IntoIden + Copy + 'static,
    window: ReleaseWindow,
) -> SimpleExpr {
    let subquery = Query::select()
        .expr(Expr::val(1))
        .from_as(table, alias)
        .and_where(Expr::col((alias, recorded_at)).equals((WriteRecords::Table, WriteRecords::RecordedAt)))
        .and_where(Expr::col((alias, write_id)).equals((WriteRecords::Table, WriteRecords::WriteId)))
        .cond_where(window.condition((alias, recorded_at)))
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
    use crate::{
        history_chunk_span::HistoryChunkSpan,
        write_record_release::{Ah, ReleaseWindow, Sh, Ti, base_release, named_window, no_history_reference, no_intent_reference, sweep_windows},
    };
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use andromeda_postgresql_schema::{
        queue::TemporalAppendIntents,
        temporal::{AttributeHistory, ScopeHistory},
    };
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("valid instant").get()
    }

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
            ReleaseWindow::default(),
        ));
        assert!(
            attribute.contains(
                r#"NOT EXISTS(SELECT 1 FROM "attribute_history" AS "ah" WHERE "ah"."recorded_at" = "write_records"."recorded_at" AND "ah"."write_id" = "write_records"."write_id")"#
            ),
            "{attribute}"
        );
        let scope = render(no_history_reference(
            ScopeHistory::Table,
            ScopeHistory::WriteId,
            ScopeHistory::RecordedAt,
            Sh,
            ReleaseWindow::default(),
        ));
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
        let sql = base_release(ReleaseWindow::default()).to_string(PostgresQueryBuilder);
        assert_eq!(sql.matches("NOT EXISTS").count(), 3, "{sql}");
        for relation in ["attribute_history", "scope_history", "temporal_append_intents"] {
            assert!(sql.contains(relation), "missing {relation}: {sql}");
        }
        assert!(sql.contains(r#""ti"."write_id" = "write_records"."write_id""#), "{sql}");
        assert!(sql.contains(r#""ah"."recorded_at" = "write_records"."recorded_at""#), "{sql}");
        assert!(sql.contains(r#""sh"."recorded_at" = "write_records"."recorded_at""#), "{sql}");
    }

    #[test]
    fn a_window_bounds_the_records_and_the_history_each_protection_probes() {
        let window = ReleaseWindow {
            from: Some(instant("2026-09-10T00:00:00Z")),
            before: Some(instant("2026-10-01T00:00:00Z")),
        };
        let sql = base_release(window).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"DELETE FROM "write_records" WHERE "recorded_at" >= '2026-09-10 00:00:00.000000 +00:00' AND "recorded_at" < '2026-10-01 00:00:00.000000 +00:00' AND"#), "{sql}");
        for bounded in [r#""ah"."recorded_at""#, r#""sh"."recorded_at""#] {
            assert!(sql.contains(&format!("{bounded} >= '2026-09-10 00:00:00.000000 +00:00'")), "{bounded}: {sql}");
            assert!(sql.contains(&format!("{bounded} < '2026-10-01 00:00:00.000000 +00:00'")), "{bounded}: {sql}");
        }
    }

    #[test]
    fn a_named_release_considers_the_instants_after_the_latest_converted_chunk() {
        let span = HistoryChunkSpan {
            compressed_until: Some(instant("2026-09-10T00:00:00Z")),
            history_from: Some(instant("2026-02-26T00:00:00Z")),
        };
        assert_eq!(
            named_window(span),
            ReleaseWindow {
                from: Some(instant("2026-09-10T00:00:00Z")),
                before: None,
            }
        );
        assert_eq!(named_window(HistoryChunkSpan::default()), ReleaseWindow::default());
    }

    #[test]
    fn a_sweep_with_nothing_converted_considers_every_instant_before_the_cutoff() {
        let span = HistoryChunkSpan {
            compressed_until: None,
            history_from: Some(instant("2026-02-26T00:00:00Z")),
        };
        assert_eq!(
            sweep_windows(span, instant("2026-08-01T00:00:00Z")),
            vec![ReleaseWindow {
                from: None,
                before: Some(instant("2026-08-01T00:00:00Z")),
            }]
        );
    }

    #[test]
    fn a_sweep_skips_the_converted_chunks_and_considers_both_sides_of_them() {
        let span = HistoryChunkSpan {
            compressed_until: Some(instant("2026-09-10T00:00:00Z")),
            history_from: Some(instant("2026-02-26T00:00:00Z")),
        };
        assert_eq!(
            sweep_windows(span, instant("2026-09-20T00:00:00Z")),
            vec![
                ReleaseWindow {
                    from: None,
                    before: Some(instant("2026-02-26T00:00:00Z")),
                },
                ReleaseWindow {
                    from: Some(instant("2026-09-10T00:00:00Z")),
                    before: Some(instant("2026-09-20T00:00:00Z")),
                },
            ]
        );
    }

    #[test]
    fn a_sweep_cutoff_inside_the_converted_chunks_considers_only_the_instants_before_any_chunk() {
        let span = HistoryChunkSpan {
            compressed_until: Some(instant("2026-09-10T00:00:00Z")),
            history_from: Some(instant("2026-02-26T00:00:00Z")),
        };
        assert_eq!(
            sweep_windows(span, instant("2026-05-01T00:00:00Z")),
            vec![ReleaseWindow {
                from: None,
                before: Some(instant("2026-02-26T00:00:00Z")),
            }]
        );
    }
}
