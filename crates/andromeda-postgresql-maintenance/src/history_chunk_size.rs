//! On-disk size of the history chunk each temporal hypertable converts next.
//!
//! TimescaleDB converts a chunk to the columnstore under an `ExclusiveLock` on
//! it and on the converted chunk it merges into, held for the whole conversion,
//! and every `UPDATE` or `DELETE` reaching either waits that long. The
//! conversion takes about 20 to 25 s per GB the chunk holds, so the largest
//! chunk still in the rowstore says how long the next conversion will block
//! writes to month-old history, and whether `temporal.chunk_interval` suits the
//! tenant's ingest. A hypertable without the columnstore, or with nothing left
//! to convert, reports zero bytes.

use crate::{
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
};
use andromeda_postgresql_seaquery::timescale::{LargestRowstoreBytes, rowstore_chunk_size_statement};
use sea_query::{Iden, PostgresQueryBuilder};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row};
use std::str::FromStr;
use strum::{Display, EnumString, IntoStaticStr};

/// A temporal history hypertable whose chunks the columnstore converts.
///
/// The variant names render, in `snake_case`, to the exact `public` hypertable
/// identifiers tenant migrations create, so the same value is both the SQL
/// filter term and the metric label.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum HistoryTable {
    /// Attribute instance history.
    AttributeHistory,
    /// Scope history.
    ScopeHistory,
}

impl HistoryTable {
    /// Every history hypertable sampled.
    pub const ALL: [HistoryTable; 2] = [HistoryTable::AttributeHistory, HistoryTable::ScopeHistory];

    /// The exact hypertable identifier created by tenant migrations.
    #[must_use]
    pub fn relation_name(self) -> &'static str {
        self.into()
    }
}

/// The chunk of one history hypertable the columnstore converts next, by size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowstoreChunkSize {
    /// The hypertable the sample describes.
    pub table: HistoryTable,
    /// Total on-disk bytes, indexes and TOAST included, of its largest chunk
    /// not yet converted; zero when it has none or carries no columnstore.
    pub largest_bytes: u64,
}

/// Read the largest rowstore chunk of every history hypertable.
///
/// Every [`HistoryTable`] is answered, with zero bytes when the catalogue
/// returns no row for it.
///
/// # Errors
///
/// Returns [`MaintenanceError`] when the catalogue query fails, a row names a
/// hypertable outside [`HistoryTable`], or a size is negative.
pub async fn rowstore_chunk_sizes(connection: &mut PgConnection) -> Result<Vec<RowstoreChunkSize>, MaintenanceError> {
    let (sql, values) = rowstore_chunk_size_statement(&HistoryTable::ALL.map(HistoryTable::relation_name)).build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    let measured = rows
        .into_iter()
        .map(|row| {
            let name = row.try_get::<String, _>(HYPERTABLE_NAME)?;
            let table = HistoryTable::from_str(&name).map_err(MaintenanceError::UnknownTarget)?;
            let bytes = non_negative(row.try_get(LargestRowstoreBytes.unquoted())?, CatalogCounter::RowstoreChunkBytes)?;
            Ok(RowstoreChunkSize { table, largest_bytes: bytes })
        })
        .collect::<Result<Vec<_>, MaintenanceError>>()?;
    Ok(every_table(&measured))
}

/// The `hypertable_name` column the statement projects.
const HYPERTABLE_NAME: &str = "hypertable_name";

/// One sample per [`HistoryTable`], zero bytes for a table `measured` lacks.
fn every_table(measured: &[RowstoreChunkSize]) -> Vec<RowstoreChunkSize> {
    HistoryTable::ALL
        .into_iter()
        .map(|table| {
            measured
                .iter()
                .find(|sample| sample.table == table)
                .copied()
                .unwrap_or(RowstoreChunkSize { table, largest_bytes: 0 })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::history_chunk_size::{HistoryTable, RowstoreChunkSize, every_table};
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn each_history_table_name_round_trips_and_is_the_hypertable_identifier() {
        for table in HistoryTable::ALL {
            assert_eq!(HistoryTable::from_str(table.relation_name()), Ok(table));
        }
        assert_eq!(HistoryTable::AttributeHistory.relation_name(), "attribute_history");
        assert_eq!(HistoryTable::ScopeHistory.relation_name(), "scope_history");
    }

    #[test]
    fn a_history_table_without_a_rowstore_chunk_reports_zero_bytes() {
        let measured = [RowstoreChunkSize {
            table: HistoryTable::ScopeHistory,
            largest_bytes: 4096,
        }];
        assert_eq!(
            every_table(&measured),
            vec![
                RowstoreChunkSize {
                    table: HistoryTable::AttributeHistory,
                    largest_bytes: 0,
                },
                RowstoreChunkSize {
                    table: HistoryTable::ScopeHistory,
                    largest_bytes: 4096,
                },
            ]
        );
    }

    #[test]
    fn no_rows_report_every_history_table_at_zero_bytes() {
        assert_eq!(
            every_table(&[]),
            HistoryTable::ALL.map(|table| RowstoreChunkSize { table, largest_bytes: 0 }).to_vec()
        );
    }
}
