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
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{DynIden, Expr, ExprTrait, Query, SelectStatement};

/// The recorded rows of the one stream `stream` names in the branch `schema`
/// names, its grain taken from the stream row, less those a queued entry
/// replaces when `pending` unions them.
///
/// The rows are narrowed by their stream key alone, so the stream-led axis
/// and placement indexes return them in their order and a limited read stops
/// after its page.
fn recorded_stream_rows(schema: Option<&DynIden>, stream: &HistoryStream, pending: PendingEntries) -> SelectStatement {
    let mut select = Query::select();
    select
        .expr_as(stream.column(stream.evolution()), stream.evolution())
        .expr_as(stream.column(HistoryGrain::AttributeNameKey), HistoryGrain::AttributeNameKey)
        .expr_as(stream.column(HistoryGrain::DatasetKey), HistoryGrain::DatasetKey)
        .exprs(IDENTITY_COLUMNS.map(|column| Expr::col((RecordedRow, column))));
    for column in attribute_value_columns() {
        select.expr_as(Expr::col((RecordedRow, column)), column);
    }
    select
        .from(qualified(schema, AttributeHistory::Table).alias(RecordedRow))
        .and_where(Expr::col((RecordedRow, AttributeHistory::StreamKey)).eq(stream.column(HistoryGrain::StreamKey)));
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
/// by one: its recorded rows and, where the branch reads its queue, the
/// queued entries of its grain.
#[derive(Clone, Debug)]
pub struct StreamHistoryParts {
    /// The recorded rows of the stream.
    recorded: SelectStatement,
    /// The queued entries of the stream's grain the application would write,
    /// `None` where the branch reads no queue.
    pending: Option<SelectStatement>,
}

impl StreamHistoryParts {
    /// The recorded part, and every other part after it: a stream always has
    /// recorded rows to read, so a statement joining its parts starts from
    /// that one.
    pub fn into_reads(self) -> (SelectStatement, impl Iterator<Item = SelectStatement>) {
        (self.recorded, self.pending.into_iter())
    }
}

/// The attribute history of the one stream `stream` names in `branch`: its
/// recorded rows and, where the branch reads its queue, the queued entries of
/// its grain the application would write.
///
/// A stream sparse beside the dense streams of its table has no planner
/// statistic of its own, so a statement reading it bounds and orders each
/// part on a key a stream-led index leads with, and the rows come out of the
/// stream's own index range rather than an axis's BRIN bitmap.
#[must_use]
pub fn whole_stream_parts(branch: &HistoryBranch, stream: &HistoryStream) -> StreamHistoryParts {
    let schema = branch.schema();
    let pending = branch.pending();
    StreamHistoryParts {
        recorded: recorded_stream_rows(schema, stream, pending),
        pending: match pending {
            PendingEntries::Unioned => Some(pending::stream_attribute_rows(schema, stream)),
            PendingEntries::Absent => None,
        },
    }
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::history_source::{EvolutionColumns, HistoryBranch, HistoryOf, HistoryStream, stream_parts::whole_stream_parts};
    use sea_query::{Alias, DynIden, Expr, IntoIden, PostgresQueryBuilder};
    use std::iter::once;

    fn snapshot() -> DynIden {
        Alias::new("snap_1").into_iden()
    }

    /// Every read of the stream under `st` in `branch`, rendered.
    fn reads(branch: &HistoryBranch) -> Vec<String> {
        let (first, others) = whole_stream_parts(branch, &HistoryStream::under(Alias::new("st"))).into_reads();
        once(first).chain(others).map(|read| read.to_string(PostgresQueryBuilder)).collect()
    }

    /// The recorded part of one stream is narrowed by its stream key alone,
    /// so the stream-led axis and placement indexes serve it, and takes its
    /// grain from the stream row.
    #[test]
    fn a_stream_part_reads_its_recorded_rows_by_stream_key() {
        let reads = reads(&HistoryBranch::live_recorded());
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
        let reads = reads(&HistoryBranch::live());
        let [recorded, pending] = reads.as_slice() else {
            panic!("a recorded read and a queued one: {reads:?}")
        };
        assert!(
            recorded.contains(r#""pr"."entity_id" = "st"."entity_id" AND "pr"."attribute_name_key" = "st"."attribute_name_key""#),
            "{recorded}"
        );
        assert!(pending.contains(r#""pe"."entity_id" = "st"."entity_id""#), "{pending}");
        assert!(pending.contains(r#""pe"."attribute_name_key" = "st"."attribute_name_key""#), "{pending}");
        assert!(pending.contains(r#""pe"."dataset_key" = "st"."dataset_key""#), "{pending}");
    }

    /// Every recorded row is one part, each system time projected from its
    /// own column: no part is cut at the write instant, and `recorded_at`
    /// never stands in for `createdAt` or `modifiedAt`.
    #[test]
    fn a_stream_keeps_its_recorded_rows_in_one_part_with_every_system_time_as_stored() {
        let reads = reads(&HistoryBranch::recorded_in(snapshot()));
        let [sql] = reads.as_slice() else { panic!("one recorded read: {reads:?}") };
        for column in ["created_at", "modified_at", "deleted_at"] {
            assert!(sql.contains(&format!(r#""rh"."{column}" AS "{column}""#)), "{sql}");
        }
        assert!(!sql.contains("DISTINCT FROM") && !sql.contains(r#""rh"."recorded_at" AS"#), "{sql}");
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
