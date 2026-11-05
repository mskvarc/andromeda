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
//! The latest instant is the greatest of the identity row's `modified_at` and
//! `evolution_modified_at` and, on the live branch, the `recorded_at` of the
//! Entity's latest queued temporal append intent: a deletion clears
//! `modified_at`, and the tombstones it recorded may still be queued. Every
//! identity row holds a current state or a Temporal Evolution, so the greatest
//! is never NULL. The row is read as the statement writes or locks it, after
//! any writer it waited on.
//!
//! A refused write lands nothing, and its statement answers the latest instant
//! beside the Entity's identifier, so the caller stamps the write again after
//! it.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_schema::{current::Entities, queue::TemporalAppendIntents};
use andromeda_postgresql_seaquery::scalar_subquery::scalar_subquery;
use chrono::{DateTime, Utc};
use sea_query::{CaseStatement, DynIden, Expr, ExprTrait, Func, Iden, IntoIden, Keyword, Order, Query, SimpleExpr};

/// The column a statement answers a refused write's latest instant under,
/// `latest_instant`, NULL for a write that landed.
#[derive(Iden, Clone, Copy)]
#[iden = "latest_instant"]
pub(super) struct LatestInstant;

/// The column the update projects the Entity's latest queued instant under,
/// `queued_at`.
#[derive(Iden, Clone, Copy)]
#[iden = "queued_at"]
pub(super) struct QueuedAt;

/// The `recorded_at` of the latest temporal append intent queued for the
/// Entity `id` names, or `None` on a Snapshot branch, which queues no history:
/// `(SELECT temporal_append_intents.recorded_at FROM temporal_append_intents
/// WHERE temporal_append_intents.entity_id = <id> ORDER BY
/// temporal_append_intents.intent_key DESC LIMIT 1)`.
///
/// The intents of one Entity take their keys in the order its writes commit,
/// so the latest is the one with the greatest key, read backwards from the
/// per-Entity index.
pub(super) fn queued_instant(target: &ContextTarget, id: SimpleExpr) -> Option<SimpleExpr> {
    match target {
        ContextTarget::Live => Some(scalar_subquery(
            Query::select()
                .column((TemporalAppendIntents::Table, TemporalAppendIntents::RecordedAt))
                .from(TemporalAppendIntents::Table)
                .and_where(Expr::col((TemporalAppendIntents::Table, TemporalAppendIntents::EntityId)).eq(id))
                .order_by((TemporalAppendIntents::Table, TemporalAppendIntents::IntentKey), Order::Desc)
                .limit(1)
                .take(),
        )),
        ContextTarget::Snapshot(_) => None,
    }
}

/// The latest instant the identity row `row` names holds, with `queued` the
/// instant of its latest queued intent where the branch queues any:
/// `GREATEST(<row>.modified_at, <row>.evolution_modified_at[, <queued>])`.
pub(super) fn latest_instant(row: impl IntoIden, queued: Option<SimpleExpr>) -> SimpleExpr {
    let row: DynIden = row.into_iden();
    let mut instants: Vec<SimpleExpr> = vec![
        Expr::col((row.clone(), Entities::ModifiedAt)), // clone: the alias qualifies both columns
        Expr::col((row, Entities::EvolutionModifiedAt)),
    ];
    instants.extend(queued);
    Func::greatest(instants).into()
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
    use crate::entity::write::latest_instant::{latest_instant, latest_where_refused, predates, queued_instant, unless_refused};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_postgresql_schema::current::{E, Entities};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::{Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    fn accepted_at() -> DateTime<Utc> {
        "2026-11-05T10:00:00Z".parse().expect("a valid instant")
    }

    /// A deletion's tombstones may still be queued while its identity row
    /// holds no `modified_at`, so the live branch reads the latest intent.
    #[test]
    fn the_live_branch_reads_the_latest_queued_intent_of_the_entity() {
        let queued = queued_instant(&ContextTarget::Live, Expr::col((E, Entities::Id)));
        assert_eq!(
            render(latest_instant(E, queued)),
            concat!(
                r#"SELECT GREATEST("e"."modified_at", "e"."evolution_modified_at", "#,
                r#"(SELECT "temporal_append_intents"."recorded_at" FROM "temporal_append_intents" "#,
                r#"WHERE "temporal_append_intents"."entity_id" = "e"."id" "#,
                r#"ORDER BY "temporal_append_intents"."intent_key" DESC LIMIT 1))"#
            )
        );
    }

    /// A Snapshot branch records its history in the write's own transaction,
    /// so its identity rows hold every instant.
    #[test]
    fn a_snapshot_branch_reads_the_identity_row_alone() {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        let target = ContextTarget::Snapshot(SnapshotTarget::staging(key, id));
        assert!(queued_instant(&target, Expr::col((E, Entities::Id))).is_none());
        assert_eq!(
            render(latest_instant(E, None)),
            r#"SELECT GREATEST("e"."modified_at", "e"."evolution_modified_at")"#
        );
    }

    /// A write at the Entity's latest instant lands: one batch stamps one
    /// `modifiedAt` on every instance of a repeated identifier (clause
    /// 5.5.11.0).
    #[test]
    fn only_a_later_instant_predates_the_write() {
        let sql = Query::select()
            .expr(predates(latest_instant(E, None), accepted_at()))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT GREATEST("e"."modified_at", "e"."evolution_modified_at") > '2026-11-05 10:00:00.000000 +00:00'"#
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
