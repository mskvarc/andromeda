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
//! segments. `scope_history` has no stream, so its segment is the temporal
//! entity -- a scope query names entities, and one entity's scope rows in a
//! chunk are the rows it reads.
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

/// The columnstore settings one hypertable carries, as the catalogue reports
/// them.
#[derive(Clone, Debug, Eq, PartialEq)]
struct StoredColumnstore {
    /// Whether the columnstore is enabled.
    state: ColumnstoreState,
    /// The segment-by columns, empty while the columnstore is disabled.
    segment_by: Vec<String>,
    /// The order-by columns with their direction, empty while disabled.
    order_by: Vec<(String, SortDirection)>,
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
            let segment_matches = stored.segment_by.len() == 1 && stored.segment_by.first().is_some_and(|column| column == hypertable.segment_by());
            let order_matches = stored.order_by.len() == COLUMNSTORE_ORDER_BY.len()
                && stored
                    .order_by
                    .iter()
                    .zip(COLUMNSTORE_ORDER_BY)
                    .all(|((column, direction), (expected_column, expected_direction))| column == expected_column && *direction == expected_direction);
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
/// or the `ALTER TABLE` fails, [`TemporalStoreError::Decode`] when the
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
            segment_by: stored.segment_by,
            order_by: stored.order_by.into_iter().map(|(column, _)| column).collect(),
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
        .ok_or_else(|| TemporalStoreError::Decode(format!("{hypertable} is not a hypertable the columnstore can be enabled on")))?;
    let state = if row.try_get::<bool, _>("compression_enabled")? {
        ColumnstoreState::Enabled
    } else {
        ColumnstoreState::Disabled
    };
    let segment_by: Vec<String> = row.try_get::<Option<Vec<String>>, _>("segmentby")?.unwrap_or_default();
    let order_columns: Vec<String> = row.try_get::<Option<Vec<String>>, _>("orderby")?.unwrap_or_default();
    let order_descending: Vec<bool> = row.try_get::<Option<Vec<bool>>, _>("orderby_desc")?.unwrap_or_default();
    let order_by = order_columns
        .into_iter()
        .zip(order_descending)
        .map(|(column, descending)| {
            let direction = if descending { SortDirection::Descending } else { SortDirection::Ascending };
            (column, direction)
        })
        .collect();
    Ok(StoredColumnstore { state, segment_by, order_by })
}

#[cfg(test)]
mod tests {
    use crate::{
        columnstore_enablement::{ColumnstoreState, EnablementPlan, StoredColumnstore, plan_enablement},
        schema_ddl::timescale_policy::{HypertableName, SortDirection},
    };
    use pretty_assertions::assert_eq;

    /// The settings the reconciler itself would have written for `hypertable`.
    fn expected(hypertable: HypertableName) -> StoredColumnstore {
        StoredColumnstore {
            state: ColumnstoreState::Enabled,
            segment_by: vec![hypertable.segment_by().to_owned()],
            order_by: vec![
                ("recorded_at".to_owned(), SortDirection::Descending),
                ("write_id".to_owned(), SortDirection::Ascending),
                ("write_ordinal".to_owned(), SortDirection::Ascending),
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
        for hypertable in [HypertableName::AttributeHistory, HypertableName::ScopeHistory] {
            assert_eq!(plan_enablement(hypertable, &expected(hypertable)), EnablementPlan::Unchanged, "{hypertable}");
        }
    }

    #[test]
    fn a_hypertable_enabled_under_other_segment_columns_is_drift() {
        let mut other = expected(HypertableName::AttributeHistory);
        other.segment_by = vec!["temporal_entity_key".to_owned()];
        assert_eq!(plan_enablement(HypertableName::AttributeHistory, &other), EnablementPlan::Drift);
    }

    #[test]
    fn a_hypertable_enabled_under_another_order_is_drift() {
        let mut ascending = expected(HypertableName::ScopeHistory);
        ascending.order_by[0].1 = SortDirection::Ascending;
        assert_eq!(plan_enablement(HypertableName::ScopeHistory, &ascending), EnablementPlan::Drift);
        let mut shorter = expected(HypertableName::ScopeHistory);
        shorter.order_by.pop();
        assert_eq!(plan_enablement(HypertableName::ScopeHistory, &shorter), EnablementPlan::Drift);
    }
}
