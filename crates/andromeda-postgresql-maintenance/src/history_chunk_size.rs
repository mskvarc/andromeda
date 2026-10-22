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
    candidate::history_table::HistoryTable,
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
};
use andromeda_postgresql_seaquery::{
    statement::Statement,
    timescale::{LargestRowstoreBytes, rowstore_chunk_size_statement},
};
use sea_query::Iden;
use sqlx::{PgConnection, Row};
use std::str::FromStr;

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
    let rows = rowstore_chunk_size_statement(&HistoryTable::ALL.map(HistoryTable::relation_name))
        .to_query()
        .fetch_all(connection)
        .await?;
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
    use crate::{
        candidate::history_table::HistoryTable,
        history_chunk_size::{RowstoreChunkSize, every_table},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_history_table_with_a_rowstore_chunk_reports_its_bytes() {
        let measured = [RowstoreChunkSize {
            table: HistoryTable::AttributeHistory,
            largest_bytes: 4096,
        }];
        assert_eq!(every_table(&measured), measured.to_vec());
    }

    #[test]
    fn no_rows_report_every_history_table_at_zero_bytes() {
        assert_eq!(
            every_table(&[]),
            HistoryTable::ALL.map(|table| RowstoreChunkSize { table, largest_bytes: 0 }).to_vec()
        );
    }
}
