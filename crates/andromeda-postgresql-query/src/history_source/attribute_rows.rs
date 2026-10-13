//! The rows of attribute history a temporal read takes an attribute's
//! instances from, and the streams they extend.
//!
//! A row carries its grain ([`HistoryGrain`]: the Entity identifier, the
//! attribute key and the `datasetId`) beside the `attribute_history` columns
//! under their own names, so a statement reads a recorded row and a queued one
//! alike. Ordering ties keep breaking on the write identity, which a queued
//! entry carries exactly as its recorded row will.

use crate::history_source::{HistoryBranch, HistoryOf, PendingEntries, entry_values::attribute_value_columns, grain::HistoryGrain, pending};
use andromeda_postgresql_schema::temporal::{AttributeHistory, TemporalAttributeStreams, TemporalEntities};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{Condition, DynIden, Expr, ExprTrait, Iden, IntoIden, JoinType, Query, SelectStatement, SimpleExpr, TableRef, UnionType};

/// Alias `rh`: a recorded history row.
#[derive(Iden, Clone, Copy)]
#[iden = "rh"]
struct RecordedRow;

/// Alias `rs`: a recorded stream.
#[derive(Iden, Clone, Copy)]
#[iden = "rs"]
struct RecordedStream;

/// Alias `re`: a recorded Temporal Evolution.
#[derive(Iden, Clone, Copy)]
#[iden = "re"]
struct RecordedEvolution;

/// The identity columns of a history row, ahead of its value columns.
const IDENTITY_COLUMNS: [AttributeHistory; 3] = [AttributeHistory::RecordedAt, AttributeHistory::WriteId, AttributeHistory::WriteOrdinal];

/// `recorded` joined to the recorded Temporal Evolution its stream names.
fn joined_to_evolution(select: &mut SelectStatement, schema: Option<&DynIden>) {
    select.join_as(
        JoinType::Join,
        qualified(schema, TemporalEntities::Table),
        RecordedEvolution,
        Expr::col((RecordedEvolution, TemporalEntities::TemporalEntityKey)).equals((RecordedStream, TemporalAttributeStreams::TemporalEntityKey)),
    );
}

/// The grain of a recorded stream, under the source's names.
fn recorded_grain(select: &mut SelectStatement) {
    select
        .expr_as(Expr::col((RecordedEvolution, TemporalEntities::Id)), HistoryGrain::EntityId)
        .expr_as(
            Expr::col((RecordedStream, TemporalAttributeStreams::AttributeNameKey)),
            HistoryGrain::AttributeNameKey,
        )
        .expr_as(Expr::col((RecordedStream, TemporalAttributeStreams::DatasetId)), HistoryGrain::DatasetId);
}

/// The recorded rows of the branch `schema` names, less those a queued entry
/// replaces when `pending` unions them.
fn recorded_rows(schema: Option<&DynIden>, of: &HistoryOf, pending: PendingEntries) -> SelectStatement {
    let mut select = Query::select();
    recorded_grain(&mut select);
    select
        .exprs(IDENTITY_COLUMNS.map(|column| Expr::col((RecordedRow, column))))
        .exprs(attribute_value_columns().map(|column| Expr::col((RecordedRow, column))))
        .from(qualified(schema, AttributeHistory::Table).alias(RecordedRow))
        .join_as(
            JoinType::Join,
            qualified(schema, TemporalAttributeStreams::Table),
            RecordedStream,
            Expr::col((RecordedStream, TemporalAttributeStreams::StreamKey)).equals((RecordedRow, AttributeHistory::StreamKey)),
        );
    joined_to_evolution(&mut select, schema);
    of.narrow(&mut select, Expr::col((RecordedEvolution, TemporalEntities::Id)));
    if let Some(kept) = kept_beside_pending(
        pending,
        Expr::col((RecordedEvolution, TemporalEntities::Id)),
        Expr::col((RecordedStream, TemporalAttributeStreams::AttributeNameKey)),
        Expr::col((RecordedRow, AttributeHistory::VerbatimInstanceId)),
    ) {
        select.cond_where(kept);
    }
    select
}

/// The attribute history `of` reads in `branch`, under `alias`: its recorded
/// rows and, where the branch reads its queue, the queued entries the
/// application would write.
pub fn attribute_rows(branch: &HistoryBranch, of: &HistoryOf, alias: impl IntoIden) -> TableRef {
    let schema = branch.schema();
    let mut rows = recorded_rows(schema, of, branch.pending());
    match branch.pending() {
        PendingEntries::Unioned => {
            rows.union(UnionType::All, pending::attribute_rows(schema, of));
        }
        PendingEntries::Absent => {}
    }
    TableRef::SubQuery(Box::new(rows), alias.into_iden())
}

/// The queued attribute entries `of` reads that the application would write,
/// under `alias`, or `None` where `branch` reads no queue.
#[must_use]
pub fn pending_attribute_rows(branch: &HistoryBranch, of: &HistoryOf, alias: impl IntoIden) -> Option<TableRef> {
    match branch.pending() {
        PendingEntries::Unioned => Some(TableRef::SubQuery(Box::new(pending::attribute_rows(branch.schema(), of)), alias.into_iden())),
        PendingEntries::Absent => None,
    }
}

/// Whether a recorded row of the attribute `attribute_key` of the Entity
/// `entity_id` carrying the client `instanceId` `verbatim` stays beside the
/// queued entries, or `None` when `pending` reads none.
#[must_use]
pub fn kept_beside_pending(pending: PendingEntries, entity_id: SimpleExpr, attribute_key: SimpleExpr, verbatim: SimpleExpr) -> Option<Condition> {
    match pending {
        PendingEntries::Unioned => Some(pending::recorded_attribute_row_kept(entity_id, attribute_key, verbatim)),
        PendingEntries::Absent => None,
    }
}

/// The streams `of` reads in `branch`, under `alias`, each with its grain and
/// its recorded stream key, and where the branch reads its queue the grains
/// only queued entries extend, their key NULL.
pub fn stream_rows(branch: &HistoryBranch, of: &HistoryOf, alias: impl IntoIden) -> TableRef {
    let schema = branch.schema();
    let mut rows = Query::select();
    recorded_grain(&mut rows);
    rows.expr_as(Expr::col((RecordedStream, TemporalAttributeStreams::StreamKey)), HistoryGrain::StreamKey)
        .from(qualified(schema, TemporalAttributeStreams::Table).alias(RecordedStream));
    joined_to_evolution(&mut rows, schema);
    of.narrow(&mut rows, Expr::col((RecordedEvolution, TemporalEntities::Id)));
    match branch.pending() {
        PendingEntries::Unioned => {
            rows.union(UnionType::All, pending::stream_rows(schema, of));
        }
        PendingEntries::Absent => {}
    }
    TableRef::SubQuery(Box::new(rows), alias.into_iden())
}

#[cfg(test)]
mod tests {
    use crate::history_source::{
        HistoryBranch,
        HistoryOf,
        attribute_rows::{attribute_rows, stream_rows},
    };
    use sea_query::{Alias, Asterisk, DynIden, IntoIden, PostgresQueryBuilder, Query};

    fn rendered(source: sea_query::TableRef) -> String {
        Query::select().column(Asterisk).from(source).to_owned().to_string(PostgresQueryBuilder)
    }

    fn snapshot() -> DynIden {
        Alias::new("snap_1").into_iden()
    }

    #[test]
    fn the_live_attribute_rows_union_the_queued_entries_with_the_recorded_rows() {
        let sql = rendered(attribute_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("ah")));
        assert!(sql.contains(r#"FROM "attribute_history" AS "rh""#), "{sql}");
        assert!(sql.contains(r#"UNION ALL (SELECT "pe"."entity_id" AS "entity_id""#), "{sql}");
        assert!(sql.contains(r#"FROM "temporal_append_entries" AS "pe""#), "{sql}");
    }

    #[test]
    fn a_recorded_row_a_queued_entry_replaces_is_left_out() {
        let sql = rendered(attribute_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("ah")));
        assert!(
            sql.contains(
                r#"WHERE "rh"."verbatim_instance_id" IS NULL OR (NOT EXISTS(SELECT 1 FROM "temporal_append_entries" AS "pr" WHERE "pr"."entity_id" = "re"."id""#
            ),
            "{sql}"
        );
    }

    #[test]
    fn the_entity_a_source_is_read_for_narrows_both_branches() {
        let of = HistoryOf::Entity(sea_query::Expr::col((Alias::new("e"), Alias::new("id"))));
        let sql = rendered(attribute_rows(&HistoryBranch::live(), &of, Alias::new("ah")));
        assert!(sql.contains(r#""re"."id" = "e"."id""#), "{sql}");
        assert!(sql.contains(r#""pe"."entity_id" = "e"."id""#), "{sql}");
    }

    /// Over an empty queue the live branch reads its recorded rows alone,
    /// unqualified.
    #[test]
    fn the_live_branch_over_an_empty_queue_reads_its_recorded_rows_alone() {
        let sql = rendered(attribute_rows(&HistoryBranch::live_recorded(), &HistoryOf::EveryEntity, Alias::new("ah")));
        assert!(sql.contains(r#"FROM "attribute_history" AS "rh""#), "{sql}");
        assert!(!sql.contains("temporal_append") && !sql.contains("UNION"), "{sql}");
    }

    #[test]
    fn a_snapshot_reads_its_recorded_rows_alone() {
        let sql = rendered(attribute_rows(
            &HistoryBranch::recorded_in(snapshot()),
            &HistoryOf::EveryEntity,
            Alias::new("ah"),
        ));
        assert!(sql.contains(r#"FROM "snap_1"."attribute_history" AS "rh""#), "{sql}");
        assert!(!sql.contains("temporal_append"), "{sql}");
        assert!(!sql.contains("UNION"), "{sql}");
    }

    #[test]
    fn the_queued_grains_without_a_recorded_stream_join_the_streams_with_no_key() {
        let sql = rendered(stream_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("qs")));
        assert!(sql.contains(r#"NULL::bigint AS "stream_key""#), "{sql}");
        assert!(sql.contains(r#"NOT EXISTS(SELECT 1 FROM "temporal_attribute_streams" AS "ps""#), "{sql}");
        let snapshot = rendered(stream_rows(&HistoryBranch::recorded_in(snapshot()), &HistoryOf::EveryEntity, Alias::new("qs")));
        assert!(!snapshot.contains("temporal_append"), "{snapshot}");
    }
}
