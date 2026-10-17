//! The columnstore DDL and conversion procedure of the temporal hypertables.

use strum::{Display, EnumIter, IntoStaticStr};

/// The hypertables of the temporal schema whose storage policy the broker
/// reconciles.
///
/// `scope_history` is not one: it holds one event per change of an Entity's
/// Scope, and retention keeps the Scope in force at its cutoff, which a
/// dropped chunk would take with it (clause 4.18).
///
/// The name renders as the catalogue relation name, which is what every
/// `TimescaleDB` procedure and information view takes.
#[derive(Clone, Copy, Debug, Display, EnumIter, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum HypertableName {
    /// `attribute_history`, segmented by stream.
    AttributeHistory,
}

/// The order-by columns of every temporal hypertable's columnstore, and
/// whether each sorts descending: the write-deduplication key of the
/// hypertable's `uq_*_write` constraint, newest first.
pub const COLUMNSTORE_ORDER_BY: [(&str, SortDirection); 3] = [
    ("recorded_at", SortDirection::Descending),
    ("write_id", SortDirection::Ascending),
    ("write_ordinal", SortDirection::Ascending),
];

/// The span converted chunks are merged into, in days.
///
/// A whole multiple of the seven-day chunk interval, which `TimescaleDB`
/// requires of the merge span.
pub const COLUMNSTORE_MERGE_DAYS: u32 = 28;

/// The summary one sparse index keeps of each columnstore batch, spelled as
/// `timescaledb.sparse_index` and the catalogue spell it.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "lowercase")]
pub enum SparseIndexKind {
    /// The smallest and largest value in the batch, which a range predicate
    /// is checked against.
    Minmax,
    /// A bloom filter of the batch's values, which an equality predicate is
    /// checked against.
    Bloom,
}

/// One sparse index a hypertable's columnstore declares.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, derive_more::Display)]
#[display("{kind}({})", columns.join(", "))]
pub struct SparseIndex {
    /// The summary kept.
    pub kind: SparseIndexKind,
    /// The columns summarised, in order; more than one for a composite bloom.
    pub columns: &'static [&'static str],
}

/// The sparse indexes of `attribute_history`'s columnstore.
const ATTRIBUTE_HISTORY_SPARSE_INDEXES: [SparseIndex; 7] = [
    SparseIndex {
        kind: SparseIndexKind::Minmax,
        columns: &["observed_at"],
    },
    SparseIndex {
        kind: SparseIndexKind::Minmax,
        columns: &["created_at"],
    },
    SparseIndex {
        kind: SparseIndexKind::Minmax,
        columns: &["modified_at"],
    },
    SparseIndex {
        kind: SparseIndexKind::Minmax,
        columns: &["deleted_at"],
    },
    SparseIndex {
        kind: SparseIndexKind::Bloom,
        columns: &["generated_instance_id"],
    },
    SparseIndex {
        kind: SparseIndexKind::Bloom,
        columns: &["verbatim_instance_id"],
    },
    SparseIndex {
        kind: SparseIndexKind::Bloom,
        columns: &["recorded_at", "write_id", "write_ordinal"],
    },
];

/// The direction one columnstore order-by column sorts in.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SortDirection {
    /// Ascending, the default.
    Ascending,
    /// Descending.
    Descending,
}

impl HypertableName {
    /// The column the hypertable's columnstore segments by.
    ///
    /// `attribute_history` is segmented by stream: a temporal query names
    /// streams, so a segment holds exactly the rows one stream contributed to a
    /// chunk.
    #[must_use]
    pub const fn segment_by(self) -> &'static str {
        match self {
            HypertableName::AttributeHistory => "stream_key",
        }
    }

    /// The sparse indexes the hypertable's columnstore declares.
    ///
    /// For `attribute_history`: a `minmax` on each of the four semantic time
    /// axes clause 5.2.21 admits as a `timeproperty`, so a time filter skips
    /// batches outside its window; a `bloom` on each instance identifier,
    /// which clauses 5.6.14 and 5.6.15 address an instance by, so a
    /// modification or deletion decompresses only the batches that may hold
    /// it; and a composite `bloom` on the write identity. A replayed write
    /// reuses its original `recorded_at`, which may lie in a converted chunk,
    /// and its conflict check on the write identity cannot narrow by segment,
    /// since `stream_key` is not part of that key: without the bloom it
    /// decompresses every batch whose `recorded_at` range covers the instant.
    #[must_use]
    pub const fn sparse_indexes(self) -> &'static [SparseIndex] {
        match self {
            HypertableName::AttributeHistory => &ATTRIBUTE_HISTORY_SPARSE_INDEXES,
        }
    }
}

/// `ALTER TABLE hypertable SET (timescaledb.enable_columnstore, segmentby, orderby, compress_chunk_time_interval, sparse_index)`.
///
/// Every value is fixed per hypertable ([`HypertableName::segment_by`],
/// [`COLUMNSTORE_ORDER_BY`], [`COLUMNSTORE_MERGE_DAYS`] and
/// [`HypertableName::sparse_indexes`]), so nothing is interpolated from
/// outside the crate.
#[must_use]
pub fn enable_columnstore(hypertable: HypertableName) -> String {
    let order_by = COLUMNSTORE_ORDER_BY
        .iter()
        .map(|(column, direction)| match direction {
            SortDirection::Ascending => (*column).to_owned(),
            SortDirection::Descending => format!("{column} DESC"),
        })
        .collect::<Vec<String>>()
        .join(", ");
    let sparse_index = hypertable.sparse_indexes().iter().map(ToString::to_string).collect::<Vec<String>>().join(", ");
    format!(
        "ALTER TABLE {hypertable} SET (timescaledb.enable_columnstore, timescaledb.segmentby = '{segment_by}', timescaledb.orderby = '{order_by}', timescaledb.compress_chunk_time_interval = '{COLUMNSTORE_MERGE_DAYS} days', timescaledb.sparse_index = '{sparse_index}')",
        segment_by = hypertable.segment_by(),
    )
}

/// `CALL convert_to_columnstore('chunk'::regclass, if_not_columnstore => true)`.
///
/// `chunk` is the schema-qualified relation name `TimescaleDB`'s own chunk
/// catalogue reported, embedded as a string literal with any single quote
/// doubled: a procedure argument cannot be a bound parameter of a `CALL`
/// executed through this driver. A chunk already converted is left as it is.
#[must_use]
pub fn convert_to_columnstore(chunk: &str) -> String {
    format!(
        "CALL convert_to_columnstore('{}'::regclass, if_not_columnstore => true)",
        chunk.replace('\'', "''")
    )
}

#[cfg(test)]
mod tests {
    use crate::schema_ddl::timescale_policy::{HypertableName, SparseIndex, SparseIndexKind, convert_to_columnstore, enable_columnstore};
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn the_hypertable_renders_as_its_catalogue_name() {
        assert_eq!(HypertableName::AttributeHistory.to_string(), "attribute_history");
    }

    #[test]
    fn a_chunk_is_converted_by_its_catalogue_name_and_a_converted_one_is_left_alone() {
        assert_eq!(
            convert_to_columnstore("_timescaledb_internal._hyper_1_2_chunk"),
            "CALL convert_to_columnstore('_timescaledb_internal._hyper_1_2_chunk'::regclass, if_not_columnstore => true)"
        );
    }

    #[test]
    fn a_quote_in_a_chunk_name_cannot_end_the_literal() {
        assert_eq!(
            convert_to_columnstore(r#""odd'schema"."c""#),
            r#"CALL convert_to_columnstore('"odd''schema"."c"'::regclass, if_not_columnstore => true)"#
        );
    }

    #[test]
    fn enabling_the_columnstore_names_the_segment_and_order_of_each_hypertable() {
        assert_eq!(
            enable_columnstore(HypertableName::AttributeHistory),
            concat!(
                "ALTER TABLE attribute_history SET (timescaledb.enable_columnstore, timescaledb.segmentby = 'stream_key', ",
                "timescaledb.orderby = 'recorded_at DESC, write_id, write_ordinal', timescaledb.compress_chunk_time_interval = '28 days', ",
                "timescaledb.sparse_index = 'minmax(observed_at), minmax(created_at), minmax(modified_at), minmax(deleted_at), ",
                "bloom(generated_instance_id), bloom(verbatim_instance_id), bloom(recorded_at, write_id, write_ordinal)')"
            )
        );
    }

    #[test]
    fn a_sparse_index_renders_its_kind_over_its_columns_in_order() {
        let composite = SparseIndex {
            kind: SparseIndexKind::Bloom,
            columns: &["recorded_at", "write_id", "write_ordinal"],
        };
        assert_eq!(composite.to_string(), "bloom(recorded_at, write_id, write_ordinal)");
        let single = SparseIndex {
            kind: SparseIndexKind::Minmax,
            columns: &["observed_at"],
        };
        assert_eq!(single.to_string(), "minmax(observed_at)");
    }

    #[test]
    fn attribute_history_summarises_the_four_time_axes_both_instance_identifiers_and_the_write_identity() {
        let declared: Vec<String> = HypertableName::AttributeHistory.sparse_indexes().iter().map(ToString::to_string).collect();
        assert_eq!(
            declared,
            vec![
                "minmax(observed_at)",
                "minmax(created_at)",
                "minmax(modified_at)",
                "minmax(deleted_at)",
                "bloom(generated_instance_id)",
                "bloom(verbatim_instance_id)",
                "bloom(recorded_at, write_id, write_ordinal)",
            ]
        );
    }

    #[test]
    fn scope_history_is_no_hypertable() {
        assert_eq!(
            HypertableName::iter().map(|hypertable| hypertable.to_string()).collect::<Vec<_>>(),
            vec!["attribute_history"]
        );
    }
}
