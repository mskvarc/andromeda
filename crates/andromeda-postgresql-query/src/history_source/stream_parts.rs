//! The attribute history of one stream, as the parts a statement reads one
//! by one.
//!
//! A statement that orders, limits or bounds a stream's instances applies
//! that to every part before joining them: PostgreSQL leaves a union whose
//! rows name a lateral row as a subquery, so an order or a bound above it
//! would read every row of every part.

use crate::history_source::{
    HistoryBranch,
    HistoryStream,
    PendingEntries,
    attribute_rows::{IDENTITY_COLUMNS, RecordedRow},
    entry_values::attribute_value_columns,
    grain::HistoryGrain,
    instance_replacement::kept_beside_pending,
    pending,
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{branch::qualified, pg_operator::PgBinaryOperator};
use sea_query::{DynIden, Expr, ExprTrait, Query, SelectStatement, SimpleExpr};

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
        .expr_as(stream.column(stream.evolution()), stream.evolution())
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
    match write_instant_axis(axis) {
        Some(column) => parts(
            branch,
            stream,
            RecordedSlice::AtWriteInstant(column),
            Some(RecordedSlice::ApartFromWriteInstant(column)),
        ),
        None => parts(branch, stream, RecordedSlice::Whole, None),
    }
}

/// The attribute history of the one stream `stream` names in `branch`, its
/// recorded rows one part on every axis: for a statement ordering them on an
/// expression of its own, which an index of the same expression serves.
#[must_use]
pub fn whole_stream_parts(branch: &HistoryBranch, stream: &HistoryStream) -> StreamHistoryParts {
    parts(branch, stream, RecordedSlice::Whole, None)
}

/// The parts of the stream `stream` names in `branch`: the recorded rows
/// `recorded` takes, those `apart` takes, and the queued entries of its grain
/// where the branch reads its queue.
fn parts(branch: &HistoryBranch, stream: &HistoryStream, recorded: RecordedSlice, apart: Option<RecordedSlice>) -> StreamHistoryParts {
    let schema = branch.schema();
    let pending = branch.pending();
    let read = |slice| recorded_stream_rows(schema, stream, pending, slice);
    StreamHistoryParts {
        recorded: read(recorded),
        recorded_apart: apart.map(read),
        pending: match pending {
            PendingEntries::Unioned => Some(pending::stream_attribute_rows(schema, stream)),
            PendingEntries::Absent => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::history_source::{
        EvolutionColumns,
        HistoryBranch,
        HistoryOf,
        HistoryStream,
        stream_parts::{stream_attribute_parts, whole_stream_parts},
    };
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use sea_query::{Alias, DynIden, Expr, IntoIden, PostgresQueryBuilder};
    use std::iter::once;

    fn snapshot() -> DynIden {
        Alias::new("snap_1").into_iden()
    }

    /// The `attribute_history` migration, which declares the partial stream
    /// indexes of the two system-time axes.
    const MIGRATION: &str = include_str!("../../../../migrations/branch/0019_attribute_history.sql");

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

    /// A whole read keeps every recorded row in one part on every axis, the
    /// write-clock split left to the reads ordering on the axis column.
    #[test]
    fn a_whole_stream_read_keeps_its_recorded_rows_in_one_part_on_every_axis() {
        let stream = HistoryStream::under(Alias::new("st"));
        let (recorded, others) = whole_stream_parts(&HistoryBranch::live_recorded(), &stream).into_reads();
        let sql = recorded.to_string(PostgresQueryBuilder);
        assert!(others.count() == 0, "{sql}");
        assert!(sql.contains(r#""rh"."created_at" AS "created_at""#), "{sql}");
        assert!(
            !sql.contains("DISTINCT FROM") && !sql.contains(r#""rh"."recorded_at" AS "created_at""#),
            "{sql}"
        );
        let (_, live) = whole_stream_parts(&HistoryBranch::live(), &stream).into_reads();
        pretty_assertions::assert_eq!(live.count(), 1, "the live branch adds its queued part");
    }

    /// A stream read for an Evolution by key over recorded rows names the
    /// Evolution by the key its stream row carries, as the stream source does.
    #[test]
    fn a_stream_read_by_key_names_its_evolution_by_key() {
        let of = HistoryOf::Evolution(EvolutionColumns {
            id: Expr::col((Alias::new("e"), Alias::new("id"))),
            key: Expr::col((Alias::new("e"), Alias::new("entity_key"))),
        });
        let recorded = HistoryBranch::recorded_in(snapshot());
        let (keyed, _) = whole_stream_parts(&recorded, &HistoryStream::read_for(&recorded, &of, Alias::new("st"))).into_reads();
        let keyed = keyed.to_string(PostgresQueryBuilder);
        assert!(keyed.contains(r#""st"."entity_key" AS "entity_key""#), "{keyed}");
        assert!(!keyed.contains("entity_id"), "{keyed}");
        let live = HistoryBranch::live();
        let (by_id, _) = whole_stream_parts(&live, &HistoryStream::read_for(&live, &of, Alias::new("st"))).into_reads();
        let by_id = by_id.to_string(PostgresQueryBuilder);
        assert!(by_id.contains(r#""st"."entity_id" AS "entity_id""#), "{by_id}");
    }
}
