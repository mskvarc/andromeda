//! The rows of attribute history a temporal read takes an attribute's
//! instances from, and the streams they extend.
//!
//! A row carries its grain ([`HistoryGrain`]: the Entity identifier, the
//! attribute key and the `datasetId`) beside the `attribute_history` columns
//! under their own names, so a statement reads a recorded row and a queued one
//! alike. Ordering ties keep breaking on the write identity, which a queued
//! entry carries exactly as its recorded row will.

use crate::history_source::{
    EntityHistory,
    HistoryBranch,
    HistoryOf,
    PendingEntries,
    entry_values::attribute_value_columns,
    grain::HistoryGrain,
    instance_replacement::kept_beside_pending,
    pending,
};
use andromeda_postgresql_schema::{
    current::Entities,
    temporal::{AttributeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{DynIden, Expr, ExprTrait, Iden, IntoIden, JoinType, Query, SelectStatement, SimpleExpr, TableRef, UnionType};

/// Alias `rh`: a recorded history row.
#[derive(Iden, Clone, Copy)]
#[iden = "rh"]
pub(crate) struct RecordedRow;

/// Alias `rs`: a recorded stream.
#[derive(Iden, Clone, Copy)]
#[iden = "rs"]
struct RecordedStream;

/// Alias `re`: a recorded Temporal Evolution.
#[derive(Iden, Clone, Copy)]
#[iden = "re"]
struct RecordedEvolution;

/// The identity columns of a history row, ahead of its value columns.
pub(crate) const IDENTITY_COLUMNS: [AttributeHistory; 3] = [AttributeHistory::RecordedAt, AttributeHistory::WriteId, AttributeHistory::WriteOrdinal];

/// `recorded` joined to the recorded Temporal Evolution its stream names.
fn joined_to_evolution(select: &mut SelectStatement, schema: Option<&DynIden>) {
    select.join_as(
        JoinType::Join,
        qualified(schema, Entities::Table),
        RecordedEvolution,
        Expr::col((RecordedEvolution, Entities::EntityKey)).equals((RecordedStream, TemporalAttributeStreams::EntityKey)),
    );
}

/// The grain of a recorded stream, under the source's names.
fn recorded_grain(select: &mut SelectStatement) {
    select
        .expr_as(Expr::col((RecordedEvolution, Entities::Id)), HistoryGrain::EntityId)
        .expr_as(
            Expr::col((RecordedStream, TemporalAttributeStreams::AttributeNameKey)),
            HistoryGrain::AttributeNameKey,
        )
        .expr_as(Expr::col((RecordedStream, TemporalAttributeStreams::DatasetKey)), HistoryGrain::DatasetKey);
}

/// The grain of a recorded stream naming its Evolution by surrogate key, under
/// the source's names.
fn keyed_grain(select: &mut SelectStatement) {
    select
        .expr_as(Expr::col((RecordedStream, TemporalAttributeStreams::EntityKey)), HistoryGrain::EntityKey)
        .expr_as(
            Expr::col((RecordedStream, TemporalAttributeStreams::AttributeNameKey)),
            HistoryGrain::AttributeNameKey,
        )
        .expr_as(Expr::col((RecordedStream, TemporalAttributeStreams::DatasetKey)), HistoryGrain::DatasetKey);
}

/// The identity and value columns of the recorded rows of the branch `schema`
/// names, each joined to the stream it extends.
fn recorded_row_columns(select: &mut SelectStatement, schema: Option<&DynIden>) {
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
}

/// The recorded rows of the branch `schema` names, their grain naming the
/// Evolution by the surrogate key the stream carries, so no catalogue row of
/// the Evolution is read.
fn keyed_recorded_rows(schema: Option<&DynIden>) -> SelectStatement {
    let mut rows = Query::select();
    keyed_grain(&mut rows);
    recorded_row_columns(&mut rows, schema);
    rows
}

/// The recorded rows of the branch `schema` names, less those a queued entry
/// replaces when `pending` unions them.
fn recorded_rows(schema: Option<&DynIden>, of: &HistoryOf, pending: PendingEntries) -> SelectStatement {
    let mut select = Query::select();
    recorded_grain(&mut select);
    recorded_row_columns(&mut select, schema);
    joined_to_evolution(&mut select, schema);
    of.narrow(&mut select, Expr::col((RecordedEvolution, Entities::Id)));
    if let Some(kept) = kept_beside_pending(
        pending,
        Expr::col((RecordedEvolution, Entities::Id)),
        Expr::col((RecordedStream, TemporalAttributeStreams::AttributeNameKey)),
        Expr::col((RecordedRow, AttributeHistory::VerbatimInstanceId)),
    ) {
        select.cond_where(kept);
    }
    select
}

/// The Evolution `key` names, compared on the recorded stream's own key.
fn of_evolution_key(key: &SimpleExpr) -> SimpleExpr {
    // clone: the key stays with the read, which every source of it shares.
    Expr::col((RecordedStream, TemporalAttributeStreams::EntityKey)).eq(key.clone())
}

/// The attribute history `of` reads in `branch`, under `alias`: its recorded
/// rows and, where the branch reads its queue, the queued entries the
/// application would write. Read for an Evolution by key, the grain names it
/// by [`HistoryGrain::EntityKey`] in place of the identifier.
pub fn attribute_rows(branch: &HistoryBranch, of: &HistoryOf, alias: impl IntoIden) -> TableRef {
    let schema = branch.schema();
    if let Some(key) = branch.evolution_key(of) {
        let mut rows = keyed_recorded_rows(schema);
        rows.and_where(of_evolution_key(key));
        return TableRef::SubQuery(Box::new(rows), alias.into_iden());
    }
    let mut rows = recorded_rows(schema, of, branch.pending());
    match branch.pending() {
        PendingEntries::Unioned => {
            rows.union(UnionType::All, pending::attribute_rows(schema, of));
        }
        PendingEntries::Absent => {}
    }
    TableRef::SubQuery(Box::new(rows), alias.into_iden())
}

/// Every attribute history row `branch` holds, under `alias`, for a statement
/// that reads no Evolution's identifier: where the branch reads no queue, its
/// recorded rows named by surrogate key with no catalogue row read; where it
/// reads its queue, the rows of [`attribute_rows`] over every Evolution.
///
/// A recorded row stays beside the queued entries by its Evolution's
/// identifier, so a branch reading its queue keeps the catalogue join.
pub fn anonymous_attribute_rows(branch: &HistoryBranch, alias: impl IntoIden) -> TableRef {
    match branch.pending() {
        PendingEntries::Absent => TableRef::SubQuery(Box::new(keyed_recorded_rows(branch.schema())), alias.into_iden()),
        PendingEntries::Unioned => attribute_rows(branch, &HistoryOf::EveryEntity, alias),
    }
}

/// The attribute history `branch` holds for the Evolution `read` is taken
/// for, under `alias`: read for it inside, or every recorded row, named by
/// what the comparison beside reads.
pub fn entity_attribute_rows(branch: &HistoryBranch, read: &EntityHistory, alias: impl IntoIden) -> TableRef {
    match read {
        EntityHistory::Inside(of) => attribute_rows(branch, of, alias),
        EntityHistory::BesideById(_) => attribute_rows(branch, &HistoryOf::EveryEntity, alias),
        EntityHistory::BesideByKey(_) => TableRef::SubQuery(Box::new(keyed_recorded_rows(branch.schema())), alias.into_iden()),
    }
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

/// The streams `of` reads in `branch`, under `alias`, each with its grain and
/// its recorded stream key, and where the branch reads its queue the grains
/// only queued entries extend, their key NULL. Read for an Evolution by key,
/// the grain names it by [`HistoryGrain::EntityKey`].
pub fn stream_rows(branch: &HistoryBranch, of: &HistoryOf, alias: impl IntoIden) -> TableRef {
    let schema = branch.schema();
    if let Some(key) = branch.evolution_key(of) {
        let mut rows = keyed_stream_rows(schema);
        rows.and_where(of_evolution_key(key));
        return TableRef::SubQuery(Box::new(rows), alias.into_iden());
    }
    let mut rows = Query::select();
    recorded_grain(&mut rows);
    rows.expr_as(Expr::col((RecordedStream, TemporalAttributeStreams::StreamKey)), HistoryGrain::StreamKey)
        .from(qualified(schema, TemporalAttributeStreams::Table).alias(RecordedStream));
    joined_to_evolution(&mut rows, schema);
    of.narrow(&mut rows, Expr::col((RecordedEvolution, Entities::Id)));
    match branch.pending() {
        PendingEntries::Unioned => {
            rows.union(UnionType::All, pending::stream_rows(schema, of));
        }
        PendingEntries::Absent => {}
    }
    TableRef::SubQuery(Box::new(rows), alias.into_iden())
}

/// The streams `branch` holds for the Evolution `read` is taken for, under
/// `alias`, each with its grain and its recorded stream key: read for it
/// inside, or every recorded stream, named by what the comparison beside
/// reads.
pub fn entity_stream_rows(branch: &HistoryBranch, read: &EntityHistory, alias: impl IntoIden) -> TableRef {
    match read {
        EntityHistory::Inside(of) => stream_rows(branch, of, alias),
        EntityHistory::BesideById(_) => stream_rows(branch, &HistoryOf::EveryEntity, alias),
        EntityHistory::BesideByKey(_) => TableRef::SubQuery(Box::new(keyed_stream_rows(branch.schema())), alias.into_iden()),
    }
}

/// The recorded streams of the branch `schema` names, each with its grain
/// naming its Evolution by surrogate key and its stream key, so no catalogue
/// row of the Evolution is read.
fn keyed_stream_rows(schema: Option<&DynIden>) -> SelectStatement {
    let mut rows = Query::select();
    keyed_grain(&mut rows);
    rows.expr_as(Expr::col((RecordedStream, TemporalAttributeStreams::StreamKey)), HistoryGrain::StreamKey)
        .from(qualified(schema, TemporalAttributeStreams::Table).alias(RecordedStream));
    rows
}

#[cfg(test)]
mod tests {
    use crate::history_source::{
        BesideComparison,
        EntityHistory,
        EvolutionColumns,
        HistoryBranch,
        HistoryOf,
        attribute_rows::{anonymous_attribute_rows, attribute_rows, entity_attribute_rows, entity_stream_rows, stream_rows},
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Alias, Asterisk, DynIden, Expr, IntoIden, Query, SelectStatement};

    fn rendered(source: sea_query::TableRef) -> SelectStatement {
        Query::select().column(Asterisk).from(source).to_owned()
    }

    fn snapshot() -> DynIden {
        Alias::new("snap_1").into_iden()
    }

    #[test]
    fn the_live_attribute_rows_union_the_queued_entries_with_the_recorded_rows() {
        let statement = rendered(attribute_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("ah")));
        assert_statement_snapshot!(
            "attribute-rows-the-live-attribute-rows-union-the-queued-entries-with-the-recorded-rows",
            statement
        );
    }

    #[test]
    fn a_recorded_row_a_queued_entry_replaces_is_left_out() {
        let statement = rendered(attribute_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("ah")));
        assert_statement_snapshot!("attribute-rows-a-recorded-row-a-queued-entry-replaces-is-left-out", statement);
    }

    #[test]
    fn the_entity_a_source_is_read_for_narrows_both_branches() {
        let of = HistoryOf::Entity(Expr::col((Alias::new("e"), Alias::new("id"))));
        let statement = rendered(attribute_rows(&HistoryBranch::live(), &of, Alias::new("ah")));
        assert_statement_snapshot!("attribute-rows-the-entity-a-source-is-read-for-narrows-both-branches", statement);
    }

    /// Over an empty queue the live branch reads its recorded rows alone,
    /// unqualified.
    #[test]
    fn the_live_branch_over_an_empty_queue_reads_its_recorded_rows_alone() {
        let statement = rendered(attribute_rows(&HistoryBranch::live_recorded(), &HistoryOf::EveryEntity, Alias::new("ah")));
        assert_statement_snapshot!("attribute-rows-the-live-branch-over-an-empty-queue-reads-its-recorded-rows-alone", statement);
    }

    #[test]
    fn a_snapshot_reads_its_recorded_rows_alone() {
        let statement = rendered(attribute_rows(
            &HistoryBranch::recorded_in(snapshot()),
            &HistoryOf::EveryEntity,
            Alias::new("ah"),
        ));
        assert_statement_snapshot!("attribute-rows-a-snapshot-reads-its-recorded-rows-alone", statement);
    }

    fn read_for(branch: &HistoryBranch, compared: BesideComparison) -> EntityHistory {
        branch.entity_history(
            EvolutionColumns {
                id: Expr::col((Alias::new("e"), Alias::new("id"))),
                key: Expr::col((Alias::new("e"), Alias::new("entity_key"))),
            },
            compared,
        )
    }

    /// Compared beside by the Evolution's key, which the stream carries, the
    /// recorded rows need no catalogue row of their Evolution.
    #[test]
    fn the_rows_read_for_an_evolution_beside_name_it_by_key_without_the_catalogue() {
        for (index, branch) in [HistoryBranch::live_recorded(), HistoryBranch::recorded_in(snapshot())].into_iter().enumerate() {
            let statement = rendered(entity_attribute_rows(&branch, &read_for(&branch, BesideComparison::Key), Alias::new("ah")));
            assert_statement_snapshot!(
                format!(
                    "attribute-rows-the-rows-read-for-an-evolution-beside-name-it-by-key-without-the-catalogue-{}",
                    index + 1
                ),
                statement
            );
        }
    }

    #[test]
    fn the_streams_read_for_an_evolution_beside_name_it_by_key_without_the_catalogue() {
        let branch = HistoryBranch::recorded_in(snapshot());
        let statement = rendered(entity_stream_rows(&branch, &read_for(&branch, BesideComparison::Key), Alias::new("s")));
        assert_statement_snapshot!(
            "attribute-rows-the-streams-read-for-an-evolution-beside-name-it-by-key-without-the-catalogue",
            statement
        );
    }

    /// Over no queue, every row a statement reads without an identifier names
    /// its Evolution by the key its stream carries, with no catalogue row.
    #[test]
    fn every_recorded_row_read_without_an_identifier_names_its_evolution_by_key() {
        for (index, branch) in [HistoryBranch::live_recorded(), HistoryBranch::recorded_in(snapshot())].into_iter().enumerate() {
            let statement = rendered(anonymous_attribute_rows(&branch, Alias::new("ah")));
            assert_statement_snapshot!(
                format!(
                    "attribute-rows-every-recorded-row-read-without-an-identifier-names-its-evolution-by-key-{}",
                    index + 1
                ),
                statement
            );
        }
    }

    /// Reading its queue, the branch keeps or leaves out a recorded row by its
    /// Evolution's identifier, so the rows are those of every Evolution with
    /// the catalogue joined.
    #[test]
    fn the_rows_read_without_an_identifier_over_a_queue_are_every_evolutions_rows() {
        let branch = HistoryBranch::live();
        pretty_assertions::assert_eq!(
            rendered(anonymous_attribute_rows(&branch, Alias::new("ah"))),
            rendered(attribute_rows(&branch, &HistoryOf::EveryEntity, Alias::new("ah")))
        );
    }

    /// Read inside, the source is the one the Evolution's identifier narrows.
    #[test]
    fn the_rows_read_for_an_evolution_inside_are_its_narrowed_source() {
        let branch = HistoryBranch::live();
        let read = read_for(&branch, BesideComparison::Key);
        let EntityHistory::Inside(of) = &read else {
            panic!("the live branch reads its queue inside")
        };
        pretty_assertions::assert_eq!(
            rendered(entity_attribute_rows(&branch, &read, Alias::new("ah"))),
            rendered(attribute_rows(&branch, of, Alias::new("ah")))
        );
        pretty_assertions::assert_eq!(
            rendered(entity_stream_rows(&branch, &read, Alias::new("s"))),
            rendered(stream_rows(&branch, of, Alias::new("s")))
        );
    }

    /// Compared beside by identifier, the source is every recorded row with
    /// the identifier joined from the catalogue.
    #[test]
    fn the_rows_read_for_an_evolution_beside_by_identifier_are_every_recorded_row() {
        let branch = HistoryBranch::live_recorded();
        let read = read_for(&branch, BesideComparison::Identifier);
        pretty_assertions::assert_eq!(
            rendered(entity_attribute_rows(&branch, &read, Alias::new("ah"))),
            rendered(attribute_rows(&branch, &HistoryOf::EveryEntity, Alias::new("ah")))
        );
        pretty_assertions::assert_eq!(
            rendered(entity_stream_rows(&branch, &read, Alias::new("s"))),
            rendered(stream_rows(&branch, &HistoryOf::EveryEntity, Alias::new("s")))
        );
    }

    fn evolution() -> HistoryOf {
        HistoryOf::Evolution(EvolutionColumns {
            id: Expr::col((Alias::new("e"), Alias::new("id"))),
            key: Expr::col((Alias::new("e"), Alias::new("entity_key"))),
        })
    }

    /// Over recorded rows alone the rows and the streams of an Evolution are
    /// narrowed inside by the key every recorded stream carries, with no
    /// catalogue row read.
    #[test]
    fn the_history_of_an_evolution_over_recorded_rows_is_narrowed_by_key() {
        for (index, branch) in [HistoryBranch::live_recorded(), HistoryBranch::recorded_in(snapshot())].into_iter().enumerate() {
            let rows = rendered(attribute_rows(&branch, &evolution(), Alias::new("ah")));
            assert_statement_snapshot!(
                format!(
                    "attribute-rows-the-history-of-an-evolution-over-recorded-rows-is-narrowed-by-key-{}-1",
                    index + 1
                ),
                rows
            );
            let streams = rendered(stream_rows(&branch, &evolution(), Alias::new("qs")));
            assert_statement_snapshot!(
                format!(
                    "attribute-rows-the-history-of-an-evolution-over-recorded-rows-is-narrowed-by-key-{}-2",
                    index + 1
                ),
                streams
            );
        }
    }

    /// A queued entry carries no surrogate key, so over a queue the history
    /// of an Evolution is narrowed by its identifier in both parts.
    #[test]
    fn the_history_of_an_evolution_over_a_queue_is_narrowed_by_identifier() {
        let branch = HistoryBranch::live();
        let of = HistoryOf::Entity(Expr::col((Alias::new("e"), Alias::new("id"))));
        pretty_assertions::assert_eq!(
            rendered(attribute_rows(&branch, &evolution(), Alias::new("ah"))),
            rendered(attribute_rows(&branch, &of, Alias::new("ah")))
        );
        pretty_assertions::assert_eq!(
            rendered(stream_rows(&branch, &evolution(), Alias::new("qs"))),
            rendered(stream_rows(&branch, &of, Alias::new("qs")))
        );
    }

    #[test]
    fn the_queued_grains_without_a_recorded_stream_join_the_streams_with_no_key() {
        let live = rendered(stream_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("qs")));
        assert_statement_snapshot!(
            "attribute-rows-the-queued-grains-without-a-recorded-stream-join-the-streams-with-no-key-1",
            live
        );
        let recorded = rendered(stream_rows(&HistoryBranch::recorded_in(snapshot()), &HistoryOf::EveryEntity, Alias::new("qs")));
        assert_statement_snapshot!(
            "attribute-rows-the-queued-grains-without-a-recorded-stream-join-the-streams-with-no-key-2",
            recorded
        );
    }
}
