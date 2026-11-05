//! The latest instant an Entity holds, which no write of its current state
//! may precede.
//!
//! Clause 4.8 makes `modifiedAt` the instant of the last modification and
//! `createdAt` the instant of entry, so a write stamped earlier than an instant
//! the Entity holds would move its `modifiedAt` backwards and could modify an
//! instance before it was created. Every statement here refuses such a write:
//! the insert that gives an identity row holding a Temporal Evolution alone its
//! current state, the update and the delete. A create of an identifier no row
//! holds is never refused. A write at the Entity's latest instant is not
//! refused either, because every Entity and attribute one batch writes carries
//! one `modifiedAt` (clause 5.5.11.0), the instances of a repeated identifier
//! included.
//!
//! The latest instant is the greatest of the identity row's `modified_at`,
//! `evolution_modified_at` and `deleted_or_queued_at`. A deletion clears
//! `modified_at`, and its tombstones state no type, so they leave
//! `evolution_modified_at` as it was; a queued write's history reaches
//! `evolution_modified_at` only when the drain applies it. Both raise
//! `deleted_or_queued_at` to their instant. Every identity row holds a current
//! state or a Temporal Evolution, so the greatest is never NULL. All three are
//! read from the row the statement writes or locks, after any writer it waited
//! on, so a write committed after the statement's snapshot is compared with as
//! well.
//!
//! A refused write lands nothing, and its statement answers the latest instant
//! beside the Entity's identifier, so the caller stamps the write again after
//! it.

use andromeda_postgresql_schema::current::Entities;
use chrono::{DateTime, Utc};
use sea_query::{CaseStatement, DynIden, Expr, ExprTrait, Func, Iden, IntoIden, Keyword, SimpleExpr};

/// The column a statement answers a refused write's latest instant under,
/// `latest_instant`, NULL for a write that landed.
#[derive(Iden, Clone, Copy)]
#[iden = "latest_instant"]
pub(super) struct LatestInstant;

/// The latest instant the identity row `row` names holds:
/// `GREATEST(<row>.modified_at, <row>.evolution_modified_at,
/// <row>.deleted_or_queued_at)`.
pub(super) fn latest_instant(row: impl IntoIden) -> SimpleExpr {
    let row: DynIden = row.into_iden();
    Func::greatest([
        Expr::col((row.clone(), Entities::ModifiedAt)),          // clone: the alias qualifies every column
        Expr::col((row.clone(), Entities::EvolutionModifiedAt)), // clone: the alias qualifies every column
        Expr::col((row, Entities::DeletedOrQueuedAt)),
    ])
    .into()
}

/// Whether `latest` is later than `accepted_at`, the instant the write was
/// stamped at: `<latest> > $accepted_at`.
pub(super) fn predates(latest: SimpleExpr, accepted_at: DateTime<Utc>) -> SimpleExpr {
    latest.gt(Expr::val(accepted_at))
}

/// `value` where the write landed and `kept` where `refused` holds:
/// `CASE WHEN <refused> THEN <kept> ELSE <value> END`.
///
/// A write a statement refuses still meets the row in its `SET` list, so each
/// column it writes keeps the value it holds.
pub(super) fn unless_refused(refused: &SimpleExpr, kept: SimpleExpr, value: SimpleExpr) -> SimpleExpr {
    // clone: every column the write sets tests the one condition
    CaseStatement::new().case(refused.clone(), kept).finally(value).into()
}

/// `latest` where `refused` holds, NULL where the write landed:
/// `CASE WHEN <refused> THEN <latest> ELSE NULL END`.
pub(super) fn latest_where_refused(refused: SimpleExpr, latest: SimpleExpr) -> SimpleExpr {
    CaseStatement::new().case(refused, latest).finally(Keyword::Null).into()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::entity::write::latest_instant::{latest_instant, latest_where_refused, predates, unless_refused};
    use andromeda_postgresql_schema::current::E;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::{Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    fn accepted_at() -> DateTime<Utc> {
        "2026-11-05T10:00:00Z".parse().expect("a valid instant")
    }

    /// A deletion clears `modified_at` and leaves `evolution_modified_at` as
    /// it was, so its instant is read from the row beside the other two, as
    /// the statement locked it.
    #[test]
    fn the_latest_instant_is_read_from_the_identity_row_alone() {
        assert_eq!(
            render(latest_instant(E)),
            r#"SELECT GREATEST("e"."modified_at", "e"."evolution_modified_at", "e"."deleted_or_queued_at")"#
        );
    }

    /// A write at the Entity's latest instant lands: one batch stamps one
    /// `modifiedAt` on every instance of a repeated identifier (clause
    /// 5.5.11.0).
    #[test]
    fn only_a_later_instant_predates_the_write() {
        let sql = Query::select()
            .expr(predates(latest_instant(E), accepted_at()))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT GREATEST("e"."modified_at", "e"."evolution_modified_at", "e"."deleted_or_queued_at") > '2026-11-05 10:00:00.000000 +00:00'"#
        );
    }

    #[test]
    fn a_refused_write_keeps_the_stored_value_and_answers_the_latest_instant() {
        let refused: SimpleExpr = Expr::col("refused");
        assert_eq!(
            render(unless_refused(&refused, Expr::col("stored"), Expr::col("written"))),
            r#"SELECT (CASE WHEN ("refused") THEN "stored" ELSE "written" END)"#
        );
        assert_eq!(
            render(latest_where_refused(refused, Expr::col("latest"))),
            r#"SELECT (CASE WHEN ("refused") THEN "latest" ELSE NULL END)"#
        );
    }
}
