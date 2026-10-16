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
//! Merge span. `compress_chunk_time_interval` merges converted chunks back into
//! four-week spans so the compressed tail is not fragmented by the seven-day
//! write-side interval; the span is 28 rather than 30 days because
//! `TimescaleDB` requires it to be a whole multiple of the chunk interval. An
//! operator who moves the chunk interval to a value that does not divide 28
//! days must expect the merge to be refused with a warning rather than applied.
//!
//! Conversion horizon. Chunks older than thirty days by default are converted
//! (`temporal.columnstore_after`): thirty days is the horizon within which
//! history is expected to be mutated in place -- instance modification and
//! deletion, clauses 5.6.14 and 5.6.15, and the verbatim `instanceId` dedup that
//! precedes a replacing write -- and past which it is read as an immutable
//! archive. The lag is reconciled by [`crate::storage_policy`] once the
//! columnstore is enabled.
//!
//! A hypertable already carrying the columnstore under other segment or order
//! columns is refused rather than rewritten: changing the settings of a
//! hypertable that holds compressed chunks is not a change to make silently at
//! startup.

use crate::{
    error::TemporalStoreError,
    schema_ddl::timescale_policy::{COLUMNSTORE_ORDER_BY, HypertableName, SortDirection, enable_columnstore},
    storage_policy::PolicyReconciliation,
    temporal_store_invariant::TemporalStoreInvariant,
};
use andromeda_postgresql_seaquery::timescale::columnstore_settings_statement;
use sea_query::PostgresQueryBuilder;
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, PgPool, Row};

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
#[display("segmented by [{}] and ordered by [{}]", listed(segment_by), listed(order_by))]
pub struct StoredColumnstore {
    /// Whether the columnstore is enabled.
    state: ColumnstoreState,
    /// The segment-by columns, empty while the columnstore is disabled.
    segment_by: Vec<HypertableColumn>,
    /// The order-by columns with their direction, empty while disabled.
    order_by: Vec<OrderColumn>,
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
    /// The columnstore is enabled with other settings, which are not rewritten.
    Drift,
}

/// Decide what enabling the columnstore asks of `hypertable`, before any
/// statement runs.
fn plan_enablement(hypertable: HypertableName, stored: &StoredColumnstore) -> EnablementPlan {
    match stored.state {
        ColumnstoreState::Disabled => EnablementPlan::Enable,
        ColumnstoreState::Enabled => {
            let segment_matches = stored.segment_by.len() == 1
                && stored
                    .segment_by
                    .first()
                    .is_some_and(|column| AsRef::<str>::as_ref(column) == hypertable.segment_by());
            let order_matches = stored.order_by.len() == COLUMNSTORE_ORDER_BY.len()
                && stored
                    .order_by
                    .iter()
                    .zip(COLUMNSTORE_ORDER_BY)
                    .all(|(order, (expected_column, expected_direction))| {
                        AsRef::<str>::as_ref(&order.column) == expected_column && order.direction == expected_direction
                    });
            if segment_matches && order_matches {
                EnablementPlan::Unchanged
            } else {
                EnablementPlan::Drift
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
/// already enabled under other segment or order columns.
pub(crate) async fn reconcile_columnstore_enablement(pool: &PgPool, hypertable: HypertableName) -> Result<PolicyReconciliation, TemporalStoreError> {
    let stored = read_stored_columnstore(pool, hypertable).await?;
    match plan_enablement(hypertable, &stored) {
        EnablementPlan::Unchanged => Ok(PolicyReconciliation::Unchanged),
        EnablementPlan::Enable => {
            sqlx::query(AssertSqlSafe(enable_columnstore(hypertable))).execute(pool).await?;
            Ok(PolicyReconciliation::Adjusted)
        }
        EnablementPlan::Drift => Err(TemporalStoreError::ColumnstoreSettingsDrift {
            hypertable,
            observed: Box::new(stored),
        }),
    }
}

/// Read whether the columnstore is enabled on `hypertable` and with which
/// segment and order columns.
async fn read_stored_columnstore(pool: &PgPool, hypertable: HypertableName) -> Result<StoredColumnstore, TemporalStoreError> {
    let (sql, values) = columnstore_settings_statement(hypertable.into()).build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(AssertSqlSafe(sql), values)
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
    Ok(StoredColumnstore { state, segment_by, order_by })
}

#[cfg(test)]
mod tests {
    use crate::{
        columnstore_enablement::{ColumnstoreState, EnablementPlan, HypertableColumn, OrderColumn, StoredColumnstore, plan_enablement},
        schema_ddl::timescale_policy::{HypertableName, SortDirection},
    };

    fn order(column: &str, direction: SortDirection) -> OrderColumn {
        OrderColumn {
            column: HypertableColumn::from(column.to_owned()),
            direction,
        }
    }
    use pretty_assertions::assert_eq;

    /// The settings the reconciler itself would have written for `hypertable`.
    fn expected(hypertable: HypertableName) -> StoredColumnstore {
        StoredColumnstore {
            state: ColumnstoreState::Enabled,
            segment_by: vec![HypertableColumn::from(hypertable.segment_by().to_owned())],
            order_by: vec![
                order("recorded_at", SortDirection::Descending),
                order("write_id", SortDirection::Ascending),
                order("write_ordinal", SortDirection::Ascending),
            ],
        }
    }

    #[test]
    fn a_plain_hypertable_is_enabled() {
        let plain = StoredColumnstore {
            state: ColumnstoreState::Disabled,
            segment_by: Vec::new(),
            order_by: Vec::new(),
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
    fn a_hypertable_enabled_under_other_segment_columns_is_drift() {
        let mut other = expected(HypertableName::AttributeHistory);
        other.segment_by = vec![HypertableColumn::from("temporal_entity_key".to_owned())];
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

    /// A drift names every setting the catalogue reports, so an operator can
    /// see how the hypertable differs from what the broker enables.
    #[test]
    fn drifted_settings_render_every_segment_and_order_column() {
        let mut drifted = expected(HypertableName::AttributeHistory);
        drifted.segment_by = vec![HypertableColumn::from("temporal_entity_key".to_owned())];
        assert_eq!(
            drifted.to_string(),
            "segmented by [temporal_entity_key] and ordered by [recorded_at DESC, write_id, write_ordinal]"
        );
    }
}
