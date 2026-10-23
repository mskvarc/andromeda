//! Enabling the hypercore columnstore on the temporal hypertables.
//!
//! The columnstore is a TSL-licensed capability, so no migration configures it:
//! a migration runs once, and a tenant migrated on the Apache-2.0 core and later
//! opened on the TSL edition would carry its ledger entry and never gain the
//! columnstore. The storage-policy reconciler runs on every open instead, so
//! whichever edition a tenant is opened on, its hypertables end up in the shape
//! that edition supports.
//!
//! Segmentation. `attribute_history` is segmented by `stream_key`: a temporal
//! query names streams, so a segment holds exactly the rows one stream
//! contributed to the chunk and a stream-led scan touches only its own
//! segments.
//!
//! Ordering. Within a segment rows are ordered by `recorded_at DESC, write_id,
//! write_ordinal`, the write-deduplication key of each hypertable's `uq_*_write`
//! constraint, so the newest observation leads its segment and a batch's rows
//! stay contiguous.
//!
//! No merge. `compress_chunk_time_interval` is not set, so a converted chunk is
//! never folded into a converted neighbour. A merging conversion holds the
//! neighbour's lock for its whole run, and an instance modification or
//! deletion (clauses 5.6.14 and 5.6.15) waiting on it deadlocks the
//! conversion. A hypertable already carrying a merge span keeps its layout and
//! has the span cleared, which affects only later conversions.
//!
//! Sparse indexes. The per-batch summaries a converted chunk keeps are
//! declared ([`HypertableName::columnstore_layout`]) rather than derived from
//! whatever indexes the table carries at conversion, so reshaping a rowstore
//! index never silently changes which batches the archive can skip.
//!
//! Conversion horizon. Chunks older than thirty days by default are converted
//! (`temporal.columnstore_after`): thirty days is the horizon within which
//! history is expected to be mutated in place (instance modification and
//! deletion, clauses 5.6.14 and 5.6.15, and the verbatim `instanceId` dedup that
//! precedes a replacing write) and past which it is read as an immutable
//! archive. The broker converts them itself ([`crate::columnstore_conversion`]);
//! no `TimescaleDB` job is installed.
//!
//! A hypertable already carrying the columnstore under other segment or order
//! columns, or another set of sparse indexes, is refused rather than
//! rewritten: changing the settings of a hypertable that holds compressed
//! chunks is not a change to make silently at startup.

use crate::{
    bounded_lock_wait::{CommandWrite, execute_with_bounded_lock_wait},
    error::TemporalStoreError,
    hypertable_policy::HypertableName,
    storage_policy::PolicyReconciliation,
    temporal_store_invariant::TemporalStoreInvariant,
};
use andromeda_postgresql_seaquery::{
    columnstore_catalog::{
        ColumnstoreMerges,
        SparseIndexColumns,
        SparseIndexKindColumn,
        columnstore_merge_statement,
        columnstore_settings_statement,
        columnstore_sparse_index_statement,
    },
    columnstore_enablement::{ClearColumnstoreMerge, EnableColumnstore, SortDirection, SparseIndexKind},
    statement::Statement,
};
use sea_query::{DynIden, Iden};
use sqlx::{PgPool, Row};

/// Whether converting a chunk merges it into a converted neighbour.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColumnstoreMerge {
    /// A dimension stores a `compress_interval_length`.
    Merging,
    /// No dimension does.
    Unmerged,
}

/// Whether the columnstore is enabled on a hypertable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColumnstoreState {
    /// `timescaledb_information.hypertables.compression_enabled` is set.
    Enabled,
    /// The hypertable is a plain hypertable.
    Disabled,
}

/// A column of a temporal hypertable, as the `TimescaleDB` catalogue names it.
///
/// The catalogue reports whatever columns the hypertable's columnstore was
/// configured with, by the broker or by an operator, so the name is carried as
/// the catalogue gives it rather than narrowed to the columns the broker uses.
#[derive(Clone, Debug, Eq, PartialEq, derive_more::AsRef, derive_more::Display, derive_more::From)]
#[as_ref(str)]
pub struct HypertableColumn(String);

/// The summary kind of a sparse index, as the `TimescaleDB` catalogue names it.
///
/// Carried as the catalogue gives it, like [`HypertableColumn`], so a kind the
/// broker never declares is still reported as it stands.
#[derive(Clone, Debug, Eq, PartialEq, derive_more::AsRef, derive_more::Display, derive_more::From)]
#[as_ref(str)]
pub struct SparseIndexKindName(String);

/// One sparse index a columnstore declares, as the catalogue reports it.
#[derive(Clone, Debug, Eq, PartialEq, derive_more::Display)]
#[display("{kind}({})", listed(columns))]
pub struct StoredSparseIndex {
    /// The summary kept.
    kind: SparseIndexKindName,
    /// The columns summarised, in order.
    columns: Vec<HypertableColumn>,
}

impl StoredSparseIndex {
    /// Whether this is the sparse index `kind` over `columns` names: the same
    /// kind over the same columns in the same order.
    fn is(&self, kind: SparseIndexKind, columns: &[DynIden]) -> bool {
        AsRef::<str>::as_ref(&self.kind) == <&str>::from(kind)
            && self
                .columns
                .iter()
                .map(AsRef::<str>::as_ref)
                .eq(columns.iter().map(ToString::to_string).collect::<Vec<_>>().iter().map(String::as_str))
    }
}

/// One order-by column of a columnstore and the direction it sorts in.
#[derive(Clone, Debug, Eq, PartialEq, derive_more::Display)]
#[display("{column}{}", direction_suffix(*direction))]
pub struct OrderColumn {
    /// The column sorted by.
    column: HypertableColumn,
    /// The direction it sorts in.
    direction: SortDirection,
}

/// The columnstore settings one hypertable carries, as the catalogue reports
/// them.
#[derive(Clone, Debug, Eq, PartialEq, derive_more::Display)]
#[display(
    "segmented by [{}], ordered by [{}] and summarised by [{}]",
    listed(segment_by),
    listed(order_by),
    listed(sparse_indexes)
)]
pub struct StoredColumnstore {
    /// Whether the columnstore is enabled.
    state: ColumnstoreState,
    /// The segment-by columns, empty while the columnstore is disabled.
    segment_by: Vec<HypertableColumn>,
    /// The order-by columns with their direction, empty while disabled.
    order_by: Vec<OrderColumn>,
    /// The sparse indexes `timescaledb.sparse_index` declared, in catalogue
    /// order, empty while disabled or when none is declared.
    sparse_indexes: Vec<StoredSparseIndex>,
    /// Whether conversions merge chunks.
    merge: ColumnstoreMerge,
}

/// The suffix an order-by column's direction is written with.
const fn direction_suffix(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Ascending => "",
        SortDirection::Descending => " DESC",
    }
}

/// A list of catalogue entries, as a failure names them.
fn listed(entries: &[impl ToString]) -> String {
    entries.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}

/// What enabling the columnstore asks of one hypertable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EnablementPlan {
    /// The columnstore is disabled and is to be enabled.
    Enable,
    /// The columnstore is enabled with the expected settings.
    Unchanged,
    /// The columnstore is enabled with the expected layout but merges chunks,
    /// which is to stop.
    StopMerging,
    /// The columnstore is enabled with other settings, which are not rewritten.
    Drift,
}

/// Decide what enabling the columnstore asks of `hypertable`, before any
/// statement runs.
fn plan_enablement(hypertable: HypertableName, stored: &StoredColumnstore) -> EnablementPlan {
    match stored.state {
        ColumnstoreState::Disabled => EnablementPlan::Enable,
        ColumnstoreState::Enabled => {
            let layout = hypertable.columnstore_layout();
            let segment_matches = stored.segment_by.len() == 1
                && stored
                    .segment_by
                    .first()
                    .is_some_and(|column| AsRef::<str>::as_ref(column) == layout.segment_by.to_string());
            let order_matches = stored.order_by.len() == layout.order_by.len()
                && stored
                    .order_by
                    .iter()
                    .zip(&layout.order_by)
                    .all(|(order, (expected_column, expected_direction))| {
                        AsRef::<str>::as_ref(&order.column) == expected_column.to_string() && order.direction == *expected_direction
                    });
            // A set: the catalogue keeps the declaration's order, but a
            // hypertable declaring the same summaries in another order skips
            // the same batches.
            let sparse_matches = stored.sparse_indexes.len() == layout.sparse_indexes.len()
                && layout
                    .sparse_indexes
                    .iter()
                    .all(|(kind, columns)| stored.sparse_indexes.iter().any(|sparse_index| sparse_index.is(*kind, columns)));
            match (segment_matches && order_matches && sparse_matches, stored.merge) {
                (true, ColumnstoreMerge::Unmerged) => EnablementPlan::Unchanged,
                (true, ColumnstoreMerge::Merging) => EnablementPlan::StopMerging,
                (false, ColumnstoreMerge::Merging | ColumnstoreMerge::Unmerged) => EnablementPlan::Drift,
            }
        }
    }
}

/// Enable the columnstore on `hypertable` unless it already carries it.
///
/// Called only on an edition that provides the columnstore; the caller reports
/// [`PolicyReconciliation::Unsupported`] without a statement otherwise.
///
/// # Errors
///
/// Returns [`TemporalStoreError::Database`] when the catalogue cannot be read
/// or the `ALTER TABLE` fails, [`TemporalStoreError::Invariant`] when the
/// catalogue does not know `hypertable` as a hypertable, and
/// [`TemporalStoreError::ColumnstoreSettingsDrift`] when the columnstore is
/// already enabled under other segment or order columns or other sparse
/// indexes.
pub(crate) async fn reconcile_columnstore_enablement(pool: &PgPool, hypertable: HypertableName) -> Result<PolicyReconciliation, TemporalStoreError> {
    let stored = read_stored_columnstore(pool, hypertable).await?;
    match plan_enablement(hypertable, &stored) {
        EnablementPlan::Unchanged => Ok(PolicyReconciliation::Unchanged),
        EnablementPlan::Enable => {
            let enable = EnableColumnstore::new(hypertable.table(), hypertable.columnstore_layout());
            execute_with_bounded_lock_wait(pool, &CommandWrite(enable)).await?;
            Ok(PolicyReconciliation::Adjusted)
        }
        EnablementPlan::StopMerging => {
            execute_with_bounded_lock_wait(pool, &CommandWrite(ClearColumnstoreMerge::new(hypertable.table()))).await?;
            Ok(PolicyReconciliation::Adjusted)
        }
        EnablementPlan::Drift => Err(TemporalStoreError::ColumnstoreSettingsDrift {
            hypertable,
            observed: Box::new(stored),
        }),
    }
}

/// Read whether the columnstore is enabled on `hypertable`, with which segment
/// and order columns and sparse indexes, and whether it merges chunks.
async fn read_stored_columnstore(pool: &PgPool, hypertable: HypertableName) -> Result<StoredColumnstore, TemporalStoreError> {
    let row = columnstore_settings_statement(hypertable.into())
        .to_query()
        .fetch_optional(pool)
        .await?
        .ok_or(TemporalStoreInvariant::NotAHypertable(hypertable))?;
    let state = if row.try_get::<bool, _>("compression_enabled")? {
        ColumnstoreState::Enabled
    } else {
        ColumnstoreState::Disabled
    };
    let segment_by: Vec<HypertableColumn> = row
        .try_get::<Option<Vec<String>>, _>("segmentby")?
        .unwrap_or_default()
        .into_iter()
        .map(HypertableColumn::from)
        .collect();
    let order_columns: Vec<String> = row.try_get::<Option<Vec<String>>, _>("orderby")?.unwrap_or_default();
    let order_descending: Vec<bool> = row.try_get::<Option<Vec<bool>>, _>("orderby_desc")?.unwrap_or_default();
    let order_by = order_columns
        .into_iter()
        .zip(order_descending)
        .map(|(column, descending)| {
            let direction = if descending { SortDirection::Descending } else { SortDirection::Ascending };
            OrderColumn {
                column: HypertableColumn::from(column),
                direction,
            }
        })
        .collect();
    let sparse_indexes = read_stored_sparse_indexes(pool, hypertable).await?;
    let merges: bool = columnstore_merge_statement(hypertable.into())
        .to_query()
        .fetch_one(pool)
        .await?
        .try_get(ColumnstoreMerges.unquoted())?;
    Ok(StoredColumnstore {
        state,
        segment_by,
        order_by,
        sparse_indexes,
        merge: if merges { ColumnstoreMerge::Merging } else { ColumnstoreMerge::Unmerged },
    })
}

/// Read the sparse indexes declared on `hypertable`'s columnstore.
async fn read_stored_sparse_indexes(pool: &PgPool, hypertable: HypertableName) -> Result<Vec<StoredSparseIndex>, TemporalStoreError> {
    let rows = columnstore_sparse_index_statement(hypertable.into()).to_query().fetch_all(pool).await?;
    rows.iter()
        .map(|row| {
            Ok(StoredSparseIndex {
                kind: SparseIndexKindName::from(row.try_get::<String, _>(SparseIndexKindColumn.to_string().as_str())?),
                columns: row
                    .try_get::<Vec<String>, _>(SparseIndexColumns.to_string().as_str())?
                    .into_iter()
                    .map(HypertableColumn::from)
                    .collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        columnstore_enablement::{
            ColumnstoreMerge,
            ColumnstoreState,
            EnablementPlan,
            HypertableColumn,
            OrderColumn,
            SparseIndexKindName,
            StoredColumnstore,
            StoredSparseIndex,
            plan_enablement,
        },
        hypertable_policy::HypertableName,
    };
    use andromeda_postgresql_seaquery::columnstore_enablement::{SortDirection, SparseIndexKind};
    use sea_query::IntoIden;

    fn order(column: &str, direction: SortDirection) -> OrderColumn {
        OrderColumn {
            column: HypertableColumn::from(column.to_owned()),
            direction,
        }
    }
    use pretty_assertions::assert_eq;

    fn sparse_index(kind: &str, columns: &[&str]) -> StoredSparseIndex {
        StoredSparseIndex {
            kind: SparseIndexKindName::from(kind.to_owned()),
            columns: columns.iter().map(|column| HypertableColumn::from((*column).to_owned())).collect(),
        }
    }

    /// The settings the reconciler itself would have written for `hypertable`.
    fn expected(hypertable: HypertableName) -> StoredColumnstore {
        let layout = hypertable.columnstore_layout();
        StoredColumnstore {
            state: ColumnstoreState::Enabled,
            segment_by: vec![HypertableColumn::from(layout.segment_by.to_string())],
            order_by: vec![
                order("recorded_at", SortDirection::Descending),
                order("write_id", SortDirection::Ascending),
                order("write_ordinal", SortDirection::Ascending),
            ],
            sparse_indexes: layout
                .sparse_indexes
                .iter()
                .map(|(kind, columns)| {
                    let columns = columns.iter().map(ToString::to_string).collect::<Vec<_>>();
                    sparse_index(<&str>::from(*kind), &columns.iter().map(String::as_str).collect::<Vec<_>>())
                })
                .collect(),
            merge: ColumnstoreMerge::Unmerged,
        }
    }

    #[test]
    fn a_plain_hypertable_is_enabled() {
        let plain = StoredColumnstore {
            state: ColumnstoreState::Disabled,
            segment_by: Vec::new(),
            order_by: Vec::new(),
            sparse_indexes: Vec::new(),
            merge: ColumnstoreMerge::Unmerged,
        };
        assert_eq!(plan_enablement(HypertableName::AttributeHistory, &plain), EnablementPlan::Enable);
    }

    #[test]
    fn a_hypertable_carrying_the_expected_settings_is_left_alone() {
        assert_eq!(
            plan_enablement(HypertableName::AttributeHistory, &expected(HypertableName::AttributeHistory)),
            EnablementPlan::Unchanged
        );
    }

    #[test]
    fn a_hypertable_carrying_the_expected_layout_with_a_merge_span_stops_merging() {
        let mut merging = expected(HypertableName::AttributeHistory);
        merging.merge = ColumnstoreMerge::Merging;
        assert_eq!(plan_enablement(HypertableName::AttributeHistory, &merging), EnablementPlan::StopMerging);
    }

    #[test]
    fn a_merging_hypertable_under_another_layout_is_drift() {
        let mut other = expected(HypertableName::AttributeHistory);
        other.merge = ColumnstoreMerge::Merging;
        other.sparse_indexes.pop();
        assert_eq!(plan_enablement(HypertableName::AttributeHistory, &other), EnablementPlan::Drift);
    }

    #[test]
    fn a_hypertable_enabled_under_other_segment_columns_is_drift() {
        let mut other = expected(HypertableName::AttributeHistory);
        other.segment_by = vec![HypertableColumn::from("entity_key".to_owned())];
        assert_eq!(plan_enablement(HypertableName::AttributeHistory, &other), EnablementPlan::Drift);
    }

    #[test]
    fn a_hypertable_enabled_under_another_order_is_drift() {
        let mut ascending = expected(HypertableName::AttributeHistory);
        ascending.order_by[0].direction = SortDirection::Ascending;
        assert_eq!(plan_enablement(HypertableName::AttributeHistory, &ascending), EnablementPlan::Drift);
        let mut shorter = expected(HypertableName::AttributeHistory);
        shorter.order_by.pop();
        assert_eq!(plan_enablement(HypertableName::AttributeHistory, &shorter), EnablementPlan::Drift);
    }

    #[test]
    fn a_hypertable_declaring_the_same_sparse_indexes_in_another_order_is_left_alone() {
        let mut reordered = expected(HypertableName::AttributeHistory);
        reordered.sparse_indexes.reverse();
        assert_eq!(plan_enablement(HypertableName::AttributeHistory, &reordered), EnablementPlan::Unchanged);
    }

    #[test]
    fn a_hypertable_missing_a_declared_sparse_index_is_drift() {
        let mut missing = expected(HypertableName::AttributeHistory);
        missing.sparse_indexes.pop();
        assert_eq!(plan_enablement(HypertableName::AttributeHistory, &missing), EnablementPlan::Drift);
    }

    #[test]
    fn a_hypertable_declaring_no_sparse_index_is_drift() {
        let mut undeclared = expected(HypertableName::AttributeHistory);
        undeclared.sparse_indexes.clear();
        assert_eq!(plan_enablement(HypertableName::AttributeHistory, &undeclared), EnablementPlan::Drift);
    }

    #[test]
    fn a_hypertable_declaring_an_extra_sparse_index_is_drift() {
        let mut extra = expected(HypertableName::AttributeHistory);
        extra.sparse_indexes.push(sparse_index("minmax", &["value_number"]));
        assert_eq!(plan_enablement(HypertableName::AttributeHistory, &extra), EnablementPlan::Drift);
    }

    #[test]
    fn a_sparse_index_of_another_kind_is_drift() {
        let mut other_kind = expected(HypertableName::AttributeHistory);
        other_kind.sparse_indexes[0] = sparse_index("bloom", &["observed_at"]);
        assert_eq!(plan_enablement(HypertableName::AttributeHistory, &other_kind), EnablementPlan::Drift);
    }

    #[test]
    fn a_composite_sparse_index_matches_only_its_columns_in_order() {
        let composite = sparse_index("bloom", &["recorded_at", "write_id"]);
        let column = |name: &str| sea_query::Alias::new(name).into_iden();
        assert!(composite.is(SparseIndexKind::Bloom, &[column("recorded_at"), column("write_id")]));
        assert!(!composite.is(SparseIndexKind::Bloom, &[column("write_id"), column("recorded_at")]));
        assert!(!composite.is(SparseIndexKind::Minmax, &[column("recorded_at"), column("write_id")]));
    }

    /// A drift names every setting the catalogue reports, so an operator can
    /// see how the hypertable differs from what the broker enables.
    #[test]
    fn drifted_settings_render_every_segment_order_and_sparse_index_entry() {
        let mut drifted = expected(HypertableName::AttributeHistory);
        drifted.segment_by = vec![HypertableColumn::from("entity_key".to_owned())];
        drifted.sparse_indexes = vec![sparse_index("minmax", &["observed_at"]), sparse_index("bloom", &["recorded_at", "write_id"])];
        assert_eq!(
            drifted.to_string(),
            "segmented by [entity_key], ordered by [recorded_at DESC, write_id, write_ordinal] and summarised by [minmax(observed_at), bloom(recorded_at, write_id)]"
        );
    }
}
