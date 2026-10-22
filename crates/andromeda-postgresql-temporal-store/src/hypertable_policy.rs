//! The temporal hypertables whose storage policy the broker reconciles, and
//! the columnstore layout each is given.

use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::columnstore_enablement::{ColumnstoreLayout, SortDirection, SparseIndexKind};
use sea_query::{IntoIden, TableName};
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

/// The span converted chunks are merged into, in days.
///
/// A whole multiple of the seven-day chunk interval, which `TimescaleDB`
/// requires of the merge span.
const COLUMNSTORE_MERGE_DAYS: u32 = 28;

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
    ///   batches outside its window; a `bloom` on each instance identifier,
    ///   which clauses 5.6.14 and 5.6.15 address an instance by, so a
    ///   modification or deletion decompresses only the batches that may hold
    ///   it; and a composite `bloom` on the write identity. A replayed write
    ///   reuses its original `recorded_at`, which may lie in a converted chunk,
    ///   and its conflict check on the write identity cannot narrow by segment,
    ///   since the stream key is not part of that key: without the bloom it
    ///   decompresses every batch whose `recorded_at` range covers the instant.
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
                merge_days: COLUMNSTORE_MERGE_DAYS,
                sparse_indexes: vec![
                    (SparseIndexKind::Minmax, vec![AttributeHistory::ObservedAt.into_iden()]),
                    (SparseIndexKind::Minmax, vec![AttributeHistory::CreatedAt.into_iden()]),
                    (SparseIndexKind::Minmax, vec![AttributeHistory::ModifiedAt.into_iden()]),
                    (SparseIndexKind::Minmax, vec![AttributeHistory::DeletedAt.into_iden()]),
                    (SparseIndexKind::Bloom, vec![AttributeHistory::GeneratedInstanceId.into_iden()]),
                    (SparseIndexKind::Bloom, vec![AttributeHistory::VerbatimInstanceId.into_iden()]),
                    (
                        SparseIndexKind::Bloom,
                        vec![
                            AttributeHistory::RecordedAt.into_iden(),
                            AttributeHistory::WriteId.into_iden(),
                            AttributeHistory::WriteOrdinal.into_iden(),
                        ],
                    ),
                ],
            },
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
        assert_eq!(layout.merge_days, 28);
    }

    #[test]
    fn attribute_history_summarises_the_four_time_axes_both_instance_identifiers_and_the_write_identity() {
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
