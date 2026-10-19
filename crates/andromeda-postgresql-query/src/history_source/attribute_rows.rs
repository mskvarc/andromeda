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
    HistoryStream,
    PendingEntries,
    entry_values::attribute_value_columns,
    grain::HistoryGrain,
    instance_replacement::kept_beside_pending,
    pending,
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_postgresql_schema::temporal::{AttributeHistory, TemporalAttributeStreams, TemporalEntities};
use andromeda_postgresql_seaquery::{branch::qualified, pg_operator::PgBinaryOperator};
use sea_query::{DynIden, Expr, ExprTrait, Iden, IntoIden, JoinType, Query, SelectStatement, SimpleExpr, TableRef, UnionType};

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

/// The grain of a recorded stream naming its Evolution by surrogate key, under
/// the source's names.
fn keyed_grain(select: &mut SelectStatement) {
    select
        .expr_as(
            Expr::col((RecordedStream, TemporalAttributeStreams::TemporalEntityKey)),
            HistoryGrain::TemporalEntityKey,
        )
        .expr_as(
            Expr::col((RecordedStream, TemporalAttributeStreams::AttributeNameKey)),
            HistoryGrain::AttributeNameKey,
        )
        .expr_as(Expr::col((RecordedStream, TemporalAttributeStreams::DatasetId)), HistoryGrain::DatasetId);
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

/// The Evolution `key` names, compared on the recorded stream's own key.
fn of_evolution_key(key: &SimpleExpr) -> SimpleExpr {
    // clone: the key stays with the read, which every source of it shares.
    Expr::col((RecordedStream, TemporalAttributeStreams::TemporalEntityKey)).eq(key.clone())
}

/// The attribute history `of` reads in `branch`, under `alias`: its recorded
/// rows and, where the branch reads its queue, the queued entries the
/// application would write. Read for an Evolution by key, the grain names it
/// by [`HistoryGrain::TemporalEntityKey`] in place of the identifier.
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

/// The `attribute_history` column of the system time `axis` (clause 4.8),
/// which an appended row records at its own write instant, or `None` for the
/// axes a client or a deletion places.
const fn write_instant_axis(axis: TimeProp) -> Option<AttributeHistory> {
    match axis {
        TimeProp::Created => Some(AttributeHistory::CreatedAt),
        TimeProp::Modified => Some(AttributeHistory::ModifiedAt),
        TimeProp::Observed | TimeProp::Deleted => None,
    }
}

/// Which of a stream's recorded rows one read takes, by how the row's instant
/// on a system-time axis stands to its write instant.
#[derive(Clone, Copy)]
enum RecordedSlice {
    /// Every recorded row.
    Whole,
    /// The rows whose instant on the axis column is their write instant, with
    /// `recorded_at` projected under the axis column's name.
    AtWriteInstant(AttributeHistory),
    /// The rows whose instant on the axis column differs from their write
    /// instant or is absent.
    ApartFromWriteInstant(AttributeHistory),
}

impl RecordedSlice {
    /// What `value_column` reads from a recorded row of the slice.
    fn projected(self, value_column: AttributeHistory) -> SimpleExpr {
        match self {
            RecordedSlice::AtWriteInstant(axis) if axis == value_column => Expr::col((RecordedRow, AttributeHistory::RecordedAt)),
            RecordedSlice::Whole | RecordedSlice::AtWriteInstant(_) | RecordedSlice::ApartFromWriteInstant(_) => Expr::col((RecordedRow, value_column)),
        }
    }

    /// The condition a recorded row meets to belong to the slice, `None` for
    /// the whole stream.
    ///
    /// The apart condition is the predicate of the axis column's partial
    /// stream index, written as the migration writes it, so the planner
    /// matches the index to it.
    fn admits(self) -> Option<SimpleExpr> {
        match self {
            RecordedSlice::Whole => None,
            RecordedSlice::AtWriteInstant(axis) => Some(Expr::col((RecordedRow, axis)).equals((RecordedRow, AttributeHistory::RecordedAt))),
            RecordedSlice::ApartFromWriteInstant(axis) => {
                Some(Expr::col((RecordedRow, axis)).distinct_from(Expr::col((RecordedRow, AttributeHistory::RecordedAt))))
            }
        }
    }
}

/// The recorded rows of the one stream `stream` names in the branch `schema`
/// names that `slice` takes, its grain taken from the stream row, less those
/// a queued entry replaces when `pending` unions them.
///
/// The rows are narrowed by their stream key alone, so the stream-led axis
/// indexes return them in axis order and a limited read stops after its page.
fn recorded_stream_rows(schema: Option<&DynIden>, stream: &HistoryStream, pending: PendingEntries, slice: RecordedSlice) -> SelectStatement {
    let mut select = Query::select();
    select
        .expr_as(stream.column(HistoryGrain::EntityId), HistoryGrain::EntityId)
        .expr_as(stream.column(HistoryGrain::AttributeNameKey), HistoryGrain::AttributeNameKey)
        .expr_as(stream.column(HistoryGrain::DatasetId), HistoryGrain::DatasetId)
        .exprs(IDENTITY_COLUMNS.map(|column| Expr::col((RecordedRow, column))));
    for column in attribute_value_columns() {
        select.expr_as(slice.projected(column), column);
    }
    select
        .from(qualified(schema, AttributeHistory::Table).alias(RecordedRow))
        .and_where(Expr::col((RecordedRow, AttributeHistory::StreamKey)).eq(stream.column(HistoryGrain::StreamKey)));
    if let Some(admitted) = slice.admits() {
        select.and_where(admitted);
    }
    if let Some(kept) = kept_beside_pending(
        pending,
        stream.column(HistoryGrain::EntityId),
        stream.column(HistoryGrain::AttributeNameKey),
        Expr::col((RecordedRow, AttributeHistory::VerbatimInstanceId)),
    ) {
        select.cond_where(kept);
    }
    select
}

/// The attribute history of one stream, as the parts a statement reads one
/// by one.
///
/// A statement that orders, limits or bounds a stream's instances applies
/// that to every part before joining them: PostgreSQL leaves a union whose
/// rows name a lateral row as a subquery, so an order or a bound above it
/// would read every row of every part.
///
/// On `createdAt` or `modifiedAt`, which an appended row records at its write
/// instant (clause 4.8), the recorded rows are two parts: those whose instant
/// on the axis is that write instant, read on `recorded_at` through the
/// stream's write-clock index, and the rest, read through the axis column's
/// partial stream index. Together they hold every recorded row exactly once.
#[derive(Clone, Debug)]
pub struct StreamHistoryParts {
    /// The recorded rows of the stream, or on `createdAt` or `modifiedAt` the
    /// recorded rows stamped at their write instant.
    recorded: SelectStatement,
    /// On `createdAt` or `modifiedAt`, the recorded rows whose instant on the
    /// axis differs from their write instant or is absent; `None` on the
    /// other axes.
    recorded_apart: Option<SelectStatement>,
    /// The queued entries of the stream's grain the application would write,
    /// `None` where the branch reads no queue.
    pending: Option<SelectStatement>,
}

impl StreamHistoryParts {
    /// The first recorded part, and every other part after it: a stream
    /// always has recorded rows to read, so a statement joining its parts
    /// starts from that one.
    pub fn into_reads(self) -> (SelectStatement, impl Iterator<Item = SelectStatement>) {
        (self.recorded, self.recorded_apart.into_iter().chain(self.pending))
    }
}

/// The attribute history of the one stream `stream` names in `branch`, as a
/// statement ordering or bounding it on `axis` reads it: its recorded rows
/// and, where the branch reads its queue, the queued entries of its grain the
/// application would write.
///
/// A stream sparse beside the dense streams of its table has no planner
/// statistic of its own: a read bounding a system-time axis column alone is
/// planned for the mean stream and narrowed through that axis's BRIN index,
/// whose lossy bitmap covers every block of the window. Read on the write
/// clock, the same rows come out of the stream's own index range.
#[must_use]
pub fn stream_attribute_parts(branch: &HistoryBranch, stream: &HistoryStream, axis: TimeProp) -> StreamHistoryParts {
    let schema = branch.schema();
    let pending = branch.pending();
    let recorded = |slice| recorded_stream_rows(schema, stream, pending, slice);
    let (recorded, recorded_apart) = match write_instant_axis(axis) {
        Some(column) => (
            recorded(RecordedSlice::AtWriteInstant(column)),
            Some(recorded(RecordedSlice::ApartFromWriteInstant(column))),
        ),
        None => (recorded(RecordedSlice::Whole), None),
    };
    StreamHistoryParts {
        recorded,
        recorded_apart,
        pending: match pending {
            PendingEntries::Unioned => Some(pending::stream_attribute_rows(schema, stream)),
            PendingEntries::Absent => None,
        },
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
/// the grain names it by [`HistoryGrain::TemporalEntityKey`].
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
    of.narrow(&mut rows, Expr::col((RecordedEvolution, TemporalEntities::Id)));
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
        HistoryStream,
        attribute_rows::{anonymous_attribute_rows, attribute_rows, entity_attribute_rows, entity_stream_rows, stream_attribute_parts, stream_rows},
    };
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use sea_query::{Alias, Asterisk, DynIden, Expr, IntoIden, PostgresQueryBuilder, Query};
    use std::iter::once;

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
        let of = HistoryOf::Entity(Expr::col((Alias::new("e"), Alias::new("id"))));
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

    fn read_for(branch: &HistoryBranch, compared: BesideComparison) -> EntityHistory {
        branch.entity_history(
            EvolutionColumns {
                id: Expr::col((Alias::new("e"), Alias::new("id"))),
                key: Expr::col((Alias::new("e"), Alias::new("temporal_entity_key"))),
            },
            compared,
        )
    }

    /// Compared beside by the Evolution's key, which the stream carries, the
    /// recorded rows need no catalogue row of their Evolution.
    #[test]
    fn the_rows_read_for_an_evolution_beside_name_it_by_key_without_the_catalogue() {
        for (branch, history) in [
            (HistoryBranch::live_recorded(), r#"FROM "attribute_history" AS "rh""#),
            (HistoryBranch::recorded_in(snapshot()), r#"FROM "snap_1"."attribute_history" AS "rh""#),
        ] {
            let sql = rendered(entity_attribute_rows(&branch, &read_for(&branch, BesideComparison::Key), Alias::new("ah")));
            assert!(
                sql.starts_with(
                    r#"SELECT * FROM (SELECT "rs"."temporal_entity_key" AS "temporal_entity_key", "rs"."attribute_name_key" AS "attribute_name_key""#
                ),
                "{sql}"
            );
            assert!(sql.contains(history), "{sql}");
            assert!(sql.contains(r#""rs"."stream_key" = "rh"."stream_key""#), "{sql}");
            assert!(!sql.contains("temporal_entities") && !sql.contains("entity_id"), "{sql}");
        }
    }

    #[test]
    fn the_streams_read_for_an_evolution_beside_name_it_by_key_without_the_catalogue() {
        let branch = HistoryBranch::recorded_in(snapshot());
        let sql = rendered(entity_stream_rows(&branch, &read_for(&branch, BesideComparison::Key), Alias::new("s")));
        pretty_assertions::assert_eq!(
            sql,
            concat!(
                r#"SELECT * FROM (SELECT "rs"."temporal_entity_key" AS "temporal_entity_key", "rs"."attribute_name_key" AS "attribute_name_key", "#,
                r#""rs"."dataset_id" AS "dataset_id", "rs"."stream_key" AS "stream_key" FROM "snap_1"."temporal_attribute_streams" AS "rs") AS "s""#
            )
        );
    }

    /// Over no queue, every row a statement reads without an identifier names
    /// its Evolution by the key its stream carries, with no catalogue row.
    #[test]
    fn every_recorded_row_read_without_an_identifier_names_its_evolution_by_key() {
        for (branch, history) in [
            (HistoryBranch::live_recorded(), r#"FROM "attribute_history" AS "rh""#),
            (HistoryBranch::recorded_in(snapshot()), r#"FROM "snap_1"."attribute_history" AS "rh""#),
        ] {
            let sql = rendered(anonymous_attribute_rows(&branch, Alias::new("ah")));
            assert!(
                sql.starts_with(
                    r#"SELECT * FROM (SELECT "rs"."temporal_entity_key" AS "temporal_entity_key", "rs"."attribute_name_key" AS "attribute_name_key""#
                ),
                "{sql}"
            );
            assert!(sql.contains(history), "{sql}");
            assert!(sql.ends_with(r#"ON "rs"."stream_key" = "rh"."stream_key") AS "ah""#), "{sql}");
            assert!(!sql.contains("temporal_entities") && !sql.contains("entity_id"), "{sql}");
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
            key: Expr::col((Alias::new("e"), Alias::new("temporal_entity_key"))),
        })
    }

    /// Over recorded rows alone the rows and the streams of an Evolution are
    /// narrowed inside by the key every recorded stream carries, with no
    /// catalogue row read.
    #[test]
    fn the_history_of_an_evolution_over_recorded_rows_is_narrowed_by_key() {
        for (branch, history) in [
            (HistoryBranch::live_recorded(), r#"FROM "attribute_history" AS "rh""#),
            (HistoryBranch::recorded_in(snapshot()), r#"FROM "snap_1"."attribute_history" AS "rh""#),
        ] {
            let rows = rendered(attribute_rows(&branch, &evolution(), Alias::new("ah")));
            assert!(rows.contains(history), "{rows}");
            assert!(
                rows.ends_with(r#"WHERE "rs"."temporal_entity_key" = "e"."temporal_entity_key") AS "ah""#),
                "{rows}"
            );
            let streams = rendered(stream_rows(&branch, &evolution(), Alias::new("qs")));
            assert!(
                streams.ends_with(r#"WHERE "rs"."temporal_entity_key" = "e"."temporal_entity_key") AS "qs""#),
                "{streams}"
            );
            for sql in [rows, streams] {
                assert!(sql.contains(r#""rs"."temporal_entity_key" AS "temporal_entity_key""#), "{sql}");
                assert!(!sql.contains("temporal_entities") && !sql.contains("entity_id"), "{sql}");
            }
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
        let sql = rendered(stream_rows(&HistoryBranch::live(), &HistoryOf::EveryEntity, Alias::new("qs")));
        assert!(sql.contains(r#"NULL::bigint AS "stream_key""#), "{sql}");
        assert!(sql.contains(r#"NOT EXISTS(SELECT 1 FROM "temporal_attribute_streams" AS "ps""#), "{sql}");
        let snapshot = rendered(stream_rows(&HistoryBranch::recorded_in(snapshot()), &HistoryOf::EveryEntity, Alias::new("qs")));
        assert!(!snapshot.contains("temporal_append"), "{snapshot}");
    }

    /// The `attribute_history` migration, which declares the partial stream
    /// indexes of the two system-time axes.
    const MIGRATION: &str = include_str!("../../../../migrations/branch/0020_attribute_history.sql");

    /// Every read of the stream under `st` on `axis` in `branch`, rendered.
    fn reads(branch: &HistoryBranch, axis: TimeProp) -> Vec<String> {
        let (first, others) = stream_attribute_parts(branch, &HistoryStream::under(Alias::new("st")), axis).into_reads();
        once(first).chain(others).map(|read| read.to_string(PostgresQueryBuilder)).collect()
    }

    /// The recorded part of one stream is narrowed by its stream key alone,
    /// so the stream-led axis indexes serve it, and takes its grain from the
    /// stream row.
    #[test]
    fn a_stream_part_reads_its_recorded_rows_by_stream_key() {
        let reads = reads(&HistoryBranch::live_recorded(), TimeProp::Observed);
        let [sql] = reads.as_slice() else {
            panic!("one recorded read and no queue: {reads:?}")
        };
        assert!(sql.contains(r#"WHERE "rh"."stream_key" = "st"."stream_key""#), "{sql}");
        assert!(sql.contains(r#""st"."entity_id" AS "entity_id""#), "{sql}");
        assert!(sql.contains(r#""rh"."observed_at" AS "observed_at""#), "{sql}");
        assert!(!sql.contains("temporal_attribute_streams"), "{sql}");
    }

    /// On the live branch the queued part holds the entries of the stream's
    /// grain alone, and the recorded part leaves out a row a queued entry of
    /// that grain replaces.
    #[test]
    fn a_live_stream_part_narrows_its_queued_entries_to_the_grain() {
        let reads = reads(&HistoryBranch::live(), TimeProp::Observed);
        let [recorded, pending] = reads.as_slice() else {
            panic!("a recorded read and a queued one: {reads:?}")
        };
        assert!(
            recorded.contains(r#""pr"."entity_id" = "st"."entity_id" AND "pr"."attribute_name_key" = "st"."attribute_name_key""#),
            "{recorded}"
        );
        assert!(pending.contains(r#""pe"."entity_id" = "st"."entity_id""#), "{pending}");
        assert!(pending.contains(r#""pe"."attribute_name_key" = "st"."attribute_name_key""#), "{pending}");
        assert!(pending.contains(r#"("pe"."dataset_id" IS NOT DISTINCT FROM "st"."dataset_id")"#), "{pending}");
    }

    /// Clause 4.8 has the broker stamp `createdAt` and `modifiedAt` itself,
    /// at the write instant of an appended row: those rows are read on
    /// `recorded_at`, which the write-clock index orders, and every other row
    /// apart, so each recorded row is in exactly one read.
    #[test]
    fn a_stream_read_on_a_system_time_axis_splits_its_recorded_rows_at_the_write_instant() {
        for (axis, column) in [(TimeProp::Created, "created_at"), (TimeProp::Modified, "modified_at")] {
            let reads = reads(&HistoryBranch::live_recorded(), axis);
            let [at_write_instant, apart] = reads.as_slice() else {
                panic!("two recorded reads: {reads:?}")
            };
            assert!(at_write_instant.contains(&format!(r#""rh"."recorded_at" AS "{column}""#)), "{at_write_instant}");
            assert!(
                at_write_instant.contains(&format!(r#""rh"."{column}" = "rh"."recorded_at""#)),
                "{at_write_instant}"
            );
            assert!(apart.contains(&format!(r#""rh"."{column}" AS "{column}""#)), "{apart}");
            assert!(
                apart.contains(&format!(r#"("rh"."{column}" IS DISTINCT FROM "rh"."recorded_at")"#)),
                "a row with no instant on the axis falls in the apart read: {apart}"
            );
        }
    }

    /// The apart read's condition is the predicate of the axis column's
    /// partial stream index, so the planner can match the index to it.
    #[test]
    fn the_apart_read_states_the_predicate_of_its_partial_stream_index() {
        for column in ["created_at", "modified_at"] {
            let declaration = format!("on attribute_history (stream_key, {column} desc)\nwhere\n    {column} is distinct from recorded_at;");
            assert!(MIGRATION.contains(&declaration), "the migration declares {declaration}");
        }
        let reads = reads(&HistoryBranch::live_recorded(), TimeProp::Created);
        assert!(reads[1].contains(r#""rh"."created_at" IS DISTINCT FROM "rh"."recorded_at""#), "{reads:?}");
    }

    /// `observedAt` is the client's and `deletedAt` the deletion's, never the
    /// write clock's, so a read on either keeps its recorded rows whole.
    #[test]
    fn a_stream_read_on_an_axis_the_write_clock_does_not_stamp_keeps_its_recorded_rows_whole() {
        for axis in [TimeProp::Observed, TimeProp::Deleted] {
            let reads = reads(&HistoryBranch::recorded_in(snapshot()), axis);
            let [recorded] = reads.as_slice() else {
                panic!("{axis}: one recorded read: {reads:?}")
            };
            assert!(!recorded.contains("DISTINCT FROM"), "{recorded}");
            assert!(!recorded.contains(r#""rh"."recorded_at" AS"#), "{recorded}");
        }
    }
}
