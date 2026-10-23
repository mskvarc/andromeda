//! The temporal hypertables whose storage policy the broker reconciles, the
//! columnstore layout each is given, the columns each registers for chunk
//! skipping, and the density below which a chunk stays in the rowstore.

use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::columnstore_enablement::{ColumnstoreLayout, SortDirection, SparseIndexKind};
use sea_query::{DynIden, IntoIden, TableName};
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

/// The fewest rows per segment at which a converted chunk takes fewer bytes
/// than its rowstore heap and indexes.
///
/// Each segment costs its batch metadata, about 700 bytes for the
/// `attribute_history` layout, whatever it holds, and a converted row about 200
/// bytes against about 290 in the rowstore with its indexes. Measured on
/// `TimescaleDB` 2.31 with history rows of real Entities in one seven-day
/// chunk: one row per stream converted to 3.3 times its rowstore size, two to
/// 2.0 times, four to 1.33 times, six to 1.08 times, eight to 0.97 times and
/// sixteen to 0.73 times.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentBreakEven(u64);

impl SegmentBreakEven {
    /// Whether a chunk of `rows` rows over `segments` segments converts to
    /// fewer bytes than it holds in the rowstore. An empty chunk converts to
    /// nothing.
    #[must_use]
    pub const fn pays(self, rows: u64, segments: u64) -> bool {
        rows >= segments.saturating_mul(self.0)
    }
}

impl HypertableName {
    /// The hypertable as a relation name.
    #[must_use]
    pub fn table(self) -> TableName {
        match self {
            HypertableName::AttributeHistory => TableName::from(AttributeHistory::Table),
        }
    }

    /// How the hypertable's columnstore lays its batches out.
    ///
    /// For `attribute_history`:
    ///
    /// - Segmentation. Segmented by stream: a temporal query names streams, so
    ///   a segment holds exactly the rows one stream contributed to a chunk.
    /// - Ordering. Within a segment rows are ordered by the write-deduplication
    ///   key of the hypertable's `uq_*_write` constraint, newest first.
    /// - Sparse indexes. A `minmax` on each of the four semantic time axes
    ///   clause 5.2.21 admits as a `timeproperty`, so a time filter skips
    ///   batches outside its window, and a `bloom` on each instance identifier,
    ///   which clauses 5.6.14 and 5.6.15 address an instance by, so a
    ///   modification or deletion decompresses only the batches that may hold
    ///   it.
    /// - No merge. A converted chunk is never folded into a converted
    ///   neighbour: the merge locks the neighbour for the whole conversion, and
    ///   a modification or deletion waiting on it deadlocks the conversion.
    #[must_use]
    pub fn columnstore_layout(self) -> ColumnstoreLayout {
        match self {
            HypertableName::AttributeHistory => ColumnstoreLayout {
                segment_by: AttributeHistory::StreamKey.into_iden(),
                order_by: vec![
                    (AttributeHistory::RecordedAt.into_iden(), SortDirection::Descending),
                    (AttributeHistory::WriteId.into_iden(), SortDirection::Ascending),
                    (AttributeHistory::WriteOrdinal.into_iden(), SortDirection::Ascending),
                ],
                sparse_indexes: vec![
                    (SparseIndexKind::Minmax, vec![AttributeHistory::ObservedAt.into_iden()]),
                    (SparseIndexKind::Minmax, vec![AttributeHistory::CreatedAt.into_iden()]),
                    (SparseIndexKind::Minmax, vec![AttributeHistory::ModifiedAt.into_iden()]),
                    (SparseIndexKind::Minmax, vec![AttributeHistory::DeletedAt.into_iden()]),
                    (SparseIndexKind::Bloom, vec![AttributeHistory::GeneratedInstanceId.into_iden()]),
                    (SparseIndexKind::Bloom, vec![AttributeHistory::VerbatimInstanceId.into_iden()]),
                ],
            },
        }
    }

    /// The columns whose per-chunk range lets the planner exclude a converted
    /// chunk.
    ///
    /// For `attribute_history`, the semantic time axes clause 5.2.21 admits
    /// as a `timeproperty` that most rows carry: a temporal query bounds
    /// exactly one of them, and history is partitioned on the write clock,
    /// which none of them follows. `deletedAt` is not one: only a row
    /// recording a deletion carries it, so a typical chunk has no range of it
    /// to record, and every conversion would report the failed calculation.
    #[must_use]
    pub fn chunk_skipping_columns(self) -> [DynIden; 3] {
        match self {
            HypertableName::AttributeHistory => [
                AttributeHistory::ObservedAt.into_iden(),
                AttributeHistory::CreatedAt.into_iden(),
                AttributeHistory::ModifiedAt.into_iden(),
            ],
        }
    }

    /// The fewest rows per segment at which converting a chunk pays.
    #[must_use]
    pub const fn segment_break_even(self) -> SegmentBreakEven {
        match self {
            HypertableName::AttributeHistory => SegmentBreakEven(8),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::hypertable_policy::HypertableName;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn the_hypertable_renders_as_its_catalogue_name() {
        assert_eq!(HypertableName::AttributeHistory.to_string(), "attribute_history");
    }

    #[test]
    fn attribute_history_is_segmented_by_stream_and_ordered_by_the_write_identity_newest_first() {
        let layout = HypertableName::AttributeHistory.columnstore_layout();
        assert_eq!(layout.segment_by.to_string(), "stream_key");
        assert_eq!(
            layout
                .order_by
                .iter()
                .map(|(column, direction)| format!("{column} {direction:?}"))
                .collect::<Vec<_>>(),
            vec!["recorded_at Descending", "write_id Ascending", "write_ordinal Ascending"]
        );
    }

    #[test]
    fn attribute_history_summarises_the_four_time_axes_and_both_instance_identifiers() {
        let declared: Vec<String> = HypertableName::AttributeHistory
            .columnstore_layout()
            .sparse_indexes
            .iter()
            .map(|(kind, columns)| format!("{kind}({})", columns.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")))
            .collect();
        assert_eq!(
            declared,
            vec![
                "minmax(observed_at)",
                "minmax(created_at)",
                "minmax(modified_at)",
                "minmax(deleted_at)",
                "bloom(generated_instance_id)",
                "bloom(verbatim_instance_id)",
            ]
        );
    }

    #[test]
    fn attribute_history_skips_chunks_on_the_three_time_axes_every_instance_carries() {
        assert_eq!(
            HypertableName::AttributeHistory.chunk_skipping_columns().map(|column| column.to_string()),
            ["observed_at", "created_at", "modified_at"]
        );
    }

    #[test]
    fn a_chunk_pays_to_convert_from_eight_rows_a_segment() {
        let break_even = HypertableName::AttributeHistory.segment_break_even();
        assert!(break_even.pays(800, 100));
        assert!(break_even.pays(801, 100));
        assert!(!break_even.pays(799, 100));
        assert!(!break_even.pays(9_883_184 * 3 / 2, 9_883_184));
        assert!(break_even.pays(0, 0));
    }

    #[test]
    fn scope_history_is_no_hypertable() {
        assert_eq!(
            HypertableName::iter().map(|hypertable| hypertable.to_string()).collect::<Vec<_>>(),
            vec!["attribute_history"]
        );
    }
}
