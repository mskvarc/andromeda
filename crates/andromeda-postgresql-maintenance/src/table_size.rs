//! On-disk size sampling for the live schema's fixed set of base tables.
//!
//! A coarse, bounded read used only for storage-transparency gauges: it reports
//! the total and index bytes of a closed allowlist of `public` base tables, and
//! the updates each table took and how many of them were heap-only. Snapshot
//! schemas are deliberately excluded so the series count stays fixed no matter
//! how many snapshots exist.

use crate::{
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
};
use andromeda_postgresql_seaquery::maintenance::{indexes_size, total_relation_size};
use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row};
use std::str::FromStr;
use strum::{Display, EnumString, IntoStaticStr};

/// A base table whose on-disk size is reported.
///
/// The variant names render, in `snake_case`, to the exact `public` table
/// identifiers tenant migrations create, so the same value is both the SQL
/// filter term and the metric label.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum SizedTable {
    /// Entity identity, expiry and version token.
    Entities,
    /// Indexed current-entity query axes.
    EntitySearch,
    /// Current attribute instances, one row per top-level instance.
    CurrentAttributes,
    /// Registered subscriptions.
    Subscriptions,
    /// Subscription delivery counters.
    SubscriptionDeliveryState,
}

impl SizedTable {
    /// Every base table sampled for size.
    pub const ALL: [SizedTable; 5] = [
        SizedTable::Entities,
        SizedTable::EntitySearch,
        SizedTable::CurrentAttributes,
        SizedTable::Subscriptions,
        SizedTable::SubscriptionDeliveryState,
    ];

    /// The exact database identifier created by tenant migrations.
    #[must_use]
    pub fn relation_name(self) -> &'static str {
        self.into()
    }
}

/// One base table's on-disk size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TableSize {
    /// The table the sample describes.
    pub table: SizedTable,
    /// Total on-disk bytes, indexes and TOAST included.
    pub total_bytes: u64,
    /// On-disk bytes held by the table's indexes.
    pub index_bytes: u64,
    /// `n_tup_upd`: rows updated since the table's statistics were reset,
    /// heap-only updates included.
    pub updated_tuples: u64,
    /// `n_tup_hot_upd`: the updates that touched no index, because the new
    /// row version fit on the old one's page and no indexed column changed.
    pub hot_updated_tuples: u64,
}

/// Read total and index bytes and update counters for every allowlisted base
/// table in `public`.
///
/// # Errors
///
/// Returns [`MaintenanceError`] when the catalog query fails, a row names a
/// relation outside the allowlist, or a size counter is negative.
pub async fn table_sizes(connection: &mut PgConnection) -> Result<Vec<TableSize>, MaintenanceError> {
    let (sql, values) = table_sizes_statement().build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.into_iter()
        .map(|row| {
            let name = row.try_get::<String, _>(SizeColumn::Relname.as_str())?;
            let table = SizedTable::from_str(&name).map_err(MaintenanceError::UnknownTarget)?;
            Ok(TableSize {
                table,
                total_bytes: non_negative(row.try_get(SizeColumn::TotalBytes.as_str())?, CatalogCounter::TableTotalBytes)?,
                index_bytes: non_negative(row.try_get(SizeColumn::IndexBytes.as_str())?, CatalogCounter::TableIndexBytes)?,
                updated_tuples: non_negative(row.try_get(SizeColumn::UpdatedTuples.as_str())?, CatalogCounter::TableUpdatedTuples)?,
                hot_updated_tuples: non_negative(row.try_get(SizeColumn::HotUpdatedTuples.as_str())?, CatalogCounter::TableHotUpdatedTuples)?,
            })
        })
        .collect()
}

/// Cheap catalog size and update-counter query for every allowlisted base table.
fn table_sizes_statement() -> SelectStatement {
    Query::select()
        .column(Alias::new(SizeColumn::Relname.as_str()))
        .expr_as(total_relation_size(Expr::col(Alias::new("relid"))), Alias::new(SizeColumn::TotalBytes.as_str()))
        .expr_as(indexes_size(Expr::col(Alias::new("relid"))), Alias::new(SizeColumn::IndexBytes.as_str()))
        .expr_as(Expr::col(Alias::new("n_tup_upd")), Alias::new(SizeColumn::UpdatedTuples.as_str()))
        .expr_as(Expr::col(Alias::new("n_tup_hot_upd")), Alias::new(SizeColumn::HotUpdatedTuples.as_str()))
        .from(Alias::new("pg_stat_user_tables"))
        .and_where(Expr::col(Alias::new("schemaname")).eq("public"))
        .and_where(Expr::col(Alias::new("relname")).is_in(SizedTable::ALL.map(<&'static str>::from)))
        .to_owned()
}

/// Stable projected field names decoded from catalog rows.
#[derive(Clone, Copy)]
enum SizeColumn {
    /// Base-table relation-name projection.
    Relname,
    /// Total physical bytes projection.
    TotalBytes,
    /// Index physical bytes projection.
    IndexBytes,
    /// Updated-rows projection.
    UpdatedTuples,
    /// Heap-only updated-rows projection.
    HotUpdatedTuples,
}

impl SizeColumn {
    /// Render the exact projected field name.
    const fn as_str(self) -> &'static str {
        match self {
            SizeColumn::Relname => "relname",
            SizeColumn::TotalBytes => "total_bytes",
            SizeColumn::IndexBytes => "index_bytes",
            SizeColumn::UpdatedTuples => "updated_tuples",
            SizeColumn::HotUpdatedTuples => "hot_updated_tuples",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::table_size::{SizedTable, table_sizes_statement};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;
    use std::str::FromStr;

    #[test]
    fn each_table_name_round_trips_and_is_snake_case() {
        for table in SizedTable::ALL {
            assert_eq!(SizedTable::from_str(table.relation_name()), Ok(table));
        }
        assert_eq!(SizedTable::SubscriptionDeliveryState.relation_name(), "subscription_delivery_state");
    }

    #[test]
    fn the_query_reads_bounded_sizes_for_the_allowlisted_tables_only() {
        let sql = table_sizes_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains("pg_stat_user_tables"), "{sql}");
        assert!(sql.contains("pg_total_relation_size"), "{sql}");
        assert!(sql.contains("pg_indexes_size"), "{sql}");
        assert!(sql.contains("current_attributes"), "{sql}");
        assert!(sql.contains("subscription_delivery_state"), "{sql}");
    }

    #[test]
    fn size_sample_reads_update_and_hot_update_counters() {
        let sql = table_sizes_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""n_tup_upd" AS "updated_tuples""#), "{sql}");
        assert!(sql.contains(r#""n_tup_hot_upd" AS "hot_updated_tuples""#), "{sql}");
        assert!(sql.contains("'entities'"), "{sql}");
        assert!(sql.contains("'entity_search'"), "{sql}");
    }
}
