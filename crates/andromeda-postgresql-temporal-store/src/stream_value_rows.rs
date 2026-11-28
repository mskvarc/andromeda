//! The history rows of a stream that carry a value, as a read of one stream's
//! rows states them.
//!
//! Every stream-led index of the history is partial, and the stream key and
//! the value kind alone imply the predicate of none of them, so a read stating
//! only those reads every row of each chunk to find a stream's few. The broker
//! stamps a `createdAt` on every instance carrying a value (clause 4.8), and a
//! modification keeps it, so the rows carrying a value are those that also
//! carry a `createdAt`, and stating that is the predicate of the stream's
//! `createdAt` index, which the read then walks.

use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_postgresql_query::history_source::axis::axis_present;
use andromeda_postgresql_stored_form::tombstone::carries_a_value;
use sea_query::{Condition, IntoIden};

/// The history rows under `row` that carry a value, each also carrying the
/// `createdAt` its stream's `createdAt` index holds it by.
pub(crate) fn stream_value_rows(row: impl IntoIden + Copy) -> Condition {
    axis_present(row, TimeProp::Created).add(carries_a_value(row))
}

#[cfg(test)]
mod tests {
    use crate::stream_value_rows::stream_value_rows;
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_temporal_query::alias::Ah;
    use sea_query::Query;

    #[test]
    fn a_row_of_a_stream_carries_a_value_and_its_created_at() {
        let statement = Query::select()
            .column((Ah, AttributeHistory::StreamKey))
            .from_as(AttributeHistory::Table, Ah)
            .cond_where(stream_value_rows(Ah))
            .to_owned();
        assert_statement_snapshot!("stream-value-rows-a-row-of-a-stream-carries-a-value-and-its-created-at", statement);
    }
}
