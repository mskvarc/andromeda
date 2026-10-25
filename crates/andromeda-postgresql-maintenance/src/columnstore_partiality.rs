//! Which converted history chunks a write has left partial.
//!
//! Any `INSERT`, `UPDATE` or `DELETE` reaching a converted chunk leaves rows
//! outside its compressed batches and marks the chunk partial. The broker does
//! this by design: instance modification and deletion on history past the
//! columnstore horizon (ETSI GS CIM 009 v1.9.1 clauses 5.6.14 and 5.6.15), the
//! deletes before a replacing write, and replays landing at their original
//! write instant. A partial chunk is read through two paths, its batches and a
//! heap their summaries do not cover, and the heap grows with every further
//! write. Recompressing it is physical repair after churn, as a rebuild is for
//! an index: it rewrites only the segments holding such rows, under a
//! `SHARE UPDATE EXCLUSIVE` lock on the chunk and its compressed relation,
//! which no read or write conflicts with, though a write to a segment being
//! rewritten waits for it.
//!
//! Only the live branch holds hypertables, and only the TSL edition converts
//! chunks, so nothing is sampled anywhere else.

use crate::{
    branch::Branch,
    candidate::history_table::HistoryTable,
    catalog_counter::{CatalogCounter, non_negative, object_id},
    columnstore_edition::{ColumnstoreEdition, columnstore_edition},
    error::MaintenanceError,
};
use andromeda_postgresql_seaquery::{
    columnstore_chunk::{ChunkOidColumn, PartialChunks, UncompressedBytes, chunk_partiality_statement, partial_chunks_statement},
    statement::Statement,
    timescale::ChunkRelationColumn,
};
use sea_query::Iden;
use sqlx::{PgConnection, Row};

/// Whether a converted chunk holds rows outside its compressed batches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChunkPartiality {
    /// It does.
    Partial,
    /// Every row is in a batch.
    Whole,
}

/// One history chunk, named by its OID, read from the catalogue in the visit
/// that acts on it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HistoryChunk {
    /// Its OID, which names it in the statement acting on it.
    pub(crate) oid: u32,
    /// Its relation as `format('%I.%I', ...)` renders it, for the lock gates
    /// and the log.
    pub(crate) relation: String,
}

/// The partial chunks of one history hypertable at one visit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ColumnstorePartiality {
    /// The hypertable.
    pub(crate) table: HistoryTable,
    /// How many of its chunks are partial.
    pub(crate) partial_chunks: u64,
    /// The bytes of the oldest partial chunk's own heap, zero when none is
    /// partial.
    pub(crate) oldest_uncompressed_bytes: u64,
}

impl ColumnstorePartiality {
    /// A hypertable with no partial chunk.
    const fn none(table: HistoryTable) -> ColumnstorePartiality {
        ColumnstorePartiality {
            table,
            partial_chunks: 0,
            oldest_uncompressed_bytes: 0,
        }
    }

    /// Whether the hypertable has a partial chunk to recompress.
    pub(crate) const fn state(self) -> ChunkPartiality {
        if self.partial_chunks > 0 {
            ChunkPartiality::Partial
        } else {
            ChunkPartiality::Whole
        }
    }
}

/// The hypertables whose partial chunks a visit samples in `branch`: every one
/// in the live branch of an edition with a columnstore, none otherwise.
pub(crate) const fn sampled_tables(branch: Branch, edition: ColumnstoreEdition) -> &'static [HistoryTable] {
    match (branch, edition) {
        (Branch::Live, ColumnstoreEdition::Licensed) => &HistoryTable::ALL,
        (Branch::Live, ColumnstoreEdition::Unlicensed) | (Branch::Snapshot(_), ColumnstoreEdition::Licensed | ColumnstoreEdition::Unlicensed) => &[],
    }
}

/// Sample the partial chunks of every history hypertable `branch` holds.
pub(crate) async fn partiality_samples(connection: &mut PgConnection, branch: Branch) -> Result<Vec<ColumnstorePartiality>, MaintenanceError> {
    let edition = match branch {
        Branch::Live => columnstore_edition(connection).await?,
        Branch::Snapshot(_) => return Ok(Vec::new()),
    };
    let mut samples = Vec::new();
    for &table in sampled_tables(branch, edition) {
        let partiality = oldest_partial_chunk(connection, table)
            .await?
            .map_or(ColumnstorePartiality::none(table), |(_, partiality)| partiality);
        samples.push(partiality);
    }
    Ok(samples)
}

/// The oldest partial chunk of `table` with the hypertable's partiality,
/// `None` when no chunk is partial.
pub(crate) async fn oldest_partial_chunk(
    connection: &mut PgConnection,
    table: HistoryTable,
) -> Result<Option<(HistoryChunk, ColumnstorePartiality)>, MaintenanceError> {
    let Some(row) = partial_chunks_statement(table.relation_name()).to_query().fetch_optional(connection).await? else {
        return Ok(None);
    };
    let chunk = HistoryChunk {
        oid: object_id(row.try_get(ChunkOidColumn.unquoted())?, CatalogCounter::ChunkOid)?,
        relation: row.try_get(ChunkRelationColumn.unquoted())?,
    };
    let partiality = ColumnstorePartiality {
        table,
        partial_chunks: non_negative(row.try_get(PartialChunks.unquoted())?, CatalogCounter::PartialChunks)?,
        oldest_uncompressed_bytes: non_negative(row.try_get(UncompressedBytes.unquoted())?, CatalogCounter::UncompressedBytes)?,
    };
    Ok(Some((chunk, partiality)))
}

/// Whether the chunk with OID `chunk` is partial now.
pub(crate) async fn chunk_partiality(connection: &mut PgConnection, chunk: u32) -> Result<ChunkPartiality, MaintenanceError> {
    let partial = chunk_partiality_statement(i64::from(chunk))
        .to_query_scalar::<bool>()
        .fetch_one(connection)
        .await?;
    Ok(if partial { ChunkPartiality::Partial } else { ChunkPartiality::Whole })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        branch::Branch,
        candidate::history_table::HistoryTable,
        columnstore_edition::ColumnstoreEdition,
        columnstore_partiality::{ChunkPartiality, ColumnstorePartiality, sampled_tables},
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use pretty_assertions::assert_eq;

    #[test]
    fn partial_chunks_are_sampled_only_in_the_live_branch_of_an_edition_with_a_columnstore() {
        let snapshot = Branch::Snapshot(SnapshotKey::try_from(3).expect("positive snapshot key"));
        assert_eq!(sampled_tables(Branch::Live, ColumnstoreEdition::Licensed), HistoryTable::ALL);
        assert_eq!(sampled_tables(Branch::Live, ColumnstoreEdition::Unlicensed), []);
        assert_eq!(sampled_tables(snapshot, ColumnstoreEdition::Licensed), []);
        assert_eq!(sampled_tables(snapshot, ColumnstoreEdition::Unlicensed), []);
    }

    #[test]
    fn a_hypertable_with_a_partial_chunk_is_partial() {
        let partial = ColumnstorePartiality {
            table: HistoryTable::AttributeHistory,
            partial_chunks: 1,
            oldest_uncompressed_bytes: 8_192,
        };
        assert_eq!(partial.state(), ChunkPartiality::Partial);
        assert_eq!(ColumnstorePartiality::none(HistoryTable::AttributeHistory).state(), ChunkPartiality::Whole);
    }
}
