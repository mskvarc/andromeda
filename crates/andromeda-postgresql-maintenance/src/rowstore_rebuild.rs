//! Which closed rowstore history chunks still hold the indexes their appends
//! built.
//!
//! A history chunk's indexes fill in write order, which leaves their pages part
//! full (64 to 79 percent on attribute history), and a chunk the columnstore
//! never converts keeps them for the life of the tenant: every chunk of an
//! edition without the columnstore, and the chunks the broker's conversion
//! sweep keeps in the rowstore because they hold too few rows per stream to
//! convert. A chunk whose range has ended receives no more appends, so one
//! rebuild packs its indexes for good: the controller rebuilds each such chunk
//! once, the oldest first, with `REINDEX TABLE CONCURRENTLY`, which takes a
//! `SHARE UPDATE EXCLUSIVE` lock no read or write conflicts with, and records
//! the rebuilt chunk in the tenant database. A chunk whose indexes hold less
//! than the profile's relation floor has too little to gain and is never a
//! candidate.
//!
//! An interrupted rebuild leaves the indexes it was building invalid beside
//! the chunk; they are read here as recovery artifacts and dropped before any
//! other action of the visit.
//!
//! Only the live branch holds hypertables, so nothing is sampled anywhere
//! else.

use crate::{
    artifact::RecoveryArtifact,
    branch::Branch,
    candidate::history_table::HistoryTable,
    catalog_counter::{CatalogCounter, non_negative, object_id},
    columnstore_partiality::HistoryChunk,
    error::MaintenanceError,
    policy::PolicyProfile,
};
use andromeda_postgresql_schema::temporal::RebuiltHistoryChunks;
use andromeda_postgresql_seaquery::{
    clock::now,
    columnstore_chunk::ChunkOidColumn,
    maintenance::{MaintenanceCommand, reindex_table_concurrently},
    rowstore_chunk::{
        AwaitingChunks,
        ChunkIndexBytes,
        InvalidIndexName,
        InvalidIndexSchema,
        awaiting_rebuild_statement,
        chunk_index_bytes_statement,
        departed_chunks_deletion,
        invalid_chunk_indexes_statement,
    },
    statement::Statement,
    timescale::{ChunkNameColumn, ChunkRangeStart, ChunkRelationColumn, ChunkSchemaColumn},
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, Iden, InsertStatement, OnConflict, Query, error::Error as SeaQueryError};
use sqlx::{PgConnection, Row};

/// Whether a closed rowstore chunk awaits its rebuild.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RebuildAwaiting {
    /// One does.
    Awaiting,
    /// Every closed rowstore chunk over the floor is rebuilt.
    None,
}

/// The closed rowstore chunks of one history hypertable awaiting their
/// rebuild at one visit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RowstoreRebuild {
    /// The hypertable.
    pub(crate) table: HistoryTable,
    /// How many of its chunks await a rebuild.
    pub(crate) awaiting_chunks: u64,
    /// The index bytes of the oldest of them, zero when none awaits one.
    pub(crate) oldest_index_bytes: u64,
}

impl RowstoreRebuild {
    /// A hypertable with no chunk awaiting a rebuild.
    const fn none(table: HistoryTable) -> RowstoreRebuild {
        RowstoreRebuild {
            table,
            awaiting_chunks: 0,
            oldest_index_bytes: 0,
        }
    }

    /// Whether the hypertable has a chunk to rebuild.
    pub(crate) const fn state(self) -> RebuildAwaiting {
        if self.awaiting_chunks > 0 {
            RebuildAwaiting::Awaiting
        } else {
            RebuildAwaiting::None
        }
    }
}

/// The chunk one rebuild works on, read from the catalogue in the visit that
/// acts on it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RebuildChunk {
    /// The chunk as the lock gates and the log name it.
    pub(crate) chunk: HistoryChunk,
    /// The schema holding the chunk relation.
    schema: String,
    /// The chunk relation's name.
    name: String,
    /// Its start on the time dimension, which names it in the tenant's
    /// record of rebuilt chunks.
    pub(crate) range_start: DateTime<Utc>,
}

impl RebuildChunk {
    /// The concurrent rebuild of every index of the chunk.
    pub(crate) fn rebuild_statement(&self) -> MaintenanceCommand {
        reindex_table_concurrently(Alias::new(&self.schema), Alias::new(&self.name))
    }
}

/// The hypertables whose closed rowstore chunks a visit samples in `branch`:
/// every one in the live branch, none in a Snapshot branch.
pub(crate) const fn sampled_tables(branch: Branch) -> &'static [HistoryTable] {
    match branch {
        Branch::Live => &HistoryTable::ALL,
        Branch::Snapshot(_) => &[],
    }
}

/// Sample the closed rowstore chunks awaiting a rebuild of every history
/// hypertable `branch` holds.
pub(crate) async fn rebuild_samples(connection: &mut PgConnection, branch: Branch, profile: PolicyProfile) -> Result<Vec<RowstoreRebuild>, MaintenanceError> {
    let mut samples = Vec::new();
    for &table in sampled_tables(branch) {
        let sample = oldest_awaiting_chunk(connection, table, profile)
            .await?
            .map_or(RowstoreRebuild::none(table), |(_, sample)| sample);
        samples.push(sample);
    }
    Ok(samples)
}

/// The oldest closed rowstore chunk of `table` awaiting a rebuild with the
/// hypertable's sample, `None` when none awaits one.
pub(crate) async fn oldest_awaiting_chunk(
    connection: &mut PgConnection,
    table: HistoryTable,
    profile: PolicyProfile,
) -> Result<Option<(RebuildChunk, RowstoreRebuild)>, MaintenanceError> {
    let floor = i64::try_from(profile.minimum_relation_bytes).unwrap_or(i64::MAX);
    let statement = awaiting_rebuild_statement(table.relation_name(), &table.kept_chunks(), &table.rebuilt_chunks(), floor);
    let Some(row) = statement.to_query().fetch_optional(connection).await? else {
        return Ok(None);
    };
    let chunk = RebuildChunk {
        chunk: HistoryChunk {
            oid: object_id(row.try_get(ChunkOidColumn.unquoted())?, CatalogCounter::ChunkOid)?,
            relation: row.try_get(ChunkRelationColumn.unquoted())?,
        },
        schema: row.try_get(ChunkSchemaColumn.unquoted())?,
        name: row.try_get(ChunkNameColumn.unquoted())?,
        range_start: row.try_get(ChunkRangeStart.unquoted())?,
    };
    let sample = RowstoreRebuild {
        table,
        awaiting_chunks: non_negative(row.try_get(AwaitingChunks.unquoted())?, CatalogCounter::AwaitingChunks)?,
        oldest_index_bytes: non_negative(row.try_get(ChunkIndexBytes.unquoted())?, CatalogCounter::ChunkIndexBytes)?,
    };
    Ok(Some((chunk, sample)))
}

/// The bytes of every index of the chunk with OID `chunk`, compared across
/// its rebuild.
pub(crate) async fn chunk_index_bytes(connection: &mut PgConnection, chunk: u32) -> Result<u64, MaintenanceError> {
    let bytes = chunk_index_bytes_statement(i64::from(chunk))
        .to_query_scalar::<i64>()
        .fetch_one(connection)
        .await?;
    non_negative(bytes, CatalogCounter::ChunkIndexBytes)
}

/// The concurrent-reindex artifacts an interrupted rebuild left on the chunks
/// of every history hypertable, the oldest chunk first. An invalid chunk
/// index of any other name is not the controller's and is left alone.
pub(crate) async fn chunk_artifacts(connection: &mut PgConnection) -> Result<Vec<RecoveryArtifact>, MaintenanceError> {
    let mut artifacts = Vec::new();
    for table in HistoryTable::ALL {
        let rows = invalid_chunk_indexes_statement(table.relation_name())
            .to_query()
            .fetch_all(&mut *connection)
            .await?;
        for row in rows {
            let chunk = HistoryChunk {
                oid: object_id(row.try_get(ChunkOidColumn.unquoted())?, CatalogCounter::ChunkOid)?,
                relation: row.try_get(ChunkRelationColumn.unquoted())?,
            };
            let name: String = row.try_get(InvalidIndexName.unquoted())?;
            artifacts.extend(RecoveryArtifact::of_chunk(table, chunk, row.try_get(InvalidIndexSchema.unquoted())?, &name));
        }
    }
    Ok(artifacts)
}

/// Record that the chunk of `table` starting at `range_start` is rebuilt, and
/// forget the rebuilt chunks retention has dropped since.
pub(crate) async fn record_rebuilt(connection: &mut PgConnection, table: HistoryTable, range_start: DateTime<Utc>) -> Result<(), MaintenanceError> {
    rebuilt_statement(table, range_start)?.to_query().execute(&mut *connection).await?;
    departed_chunks_deletion(table.relation_name(), &table.rebuilt_chunks())
        .to_query()
        .execute(connection)
        .await?;
    Ok(())
}

/// Record the chunk of `table` starting at `range_start` as rebuilt now; a
/// chunk already recorded keeps its record.
fn rebuilt_statement(table: HistoryTable, range_start: DateTime<Utc>) -> Result<InsertStatement, SeaQueryError> {
    match table {
        HistoryTable::AttributeHistory => Ok(Query::insert()
            .into_table(RebuiltHistoryChunks::Table)
            .columns([RebuiltHistoryChunks::RangeStart, RebuiltHistoryChunks::RebuiltAt])
            .values([Expr::val(range_start), now()])?
            .on_conflict(OnConflict::column(RebuiltHistoryChunks::RangeStart).do_nothing().to_owned())
            .to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        branch::Branch,
        candidate::history_table::HistoryTable,
        columnstore_partiality::HistoryChunk,
        rowstore_rebuild::{RebuildAwaiting, RebuildChunk, RowstoreRebuild, rebuilt_statement, sampled_tables},
    };
    use andromeda_context_space::snapshot_key::SnapshotKey;
    use andromeda_postgresql_seaquery::maintenance::reindex_table_concurrently;
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, PostgresQueryBuilder};

    fn instant() -> DateTime<Utc> {
        "2026-10-01T00:00:00Z".parse().expect("a valid instant")
    }

    #[test]
    fn rowstore_chunks_are_sampled_only_in_the_live_branch() {
        let snapshot = Branch::Snapshot(SnapshotKey::try_from(3).expect("positive snapshot key"));
        assert_eq!(sampled_tables(Branch::Live), HistoryTable::ALL);
        assert_eq!(sampled_tables(snapshot), []);
    }

    #[test]
    fn a_hypertable_with_a_chunk_awaiting_its_rebuild_awaits_one() {
        let awaiting = RowstoreRebuild {
            table: HistoryTable::AttributeHistory,
            awaiting_chunks: 2,
            oldest_index_bytes: 80 * 1_048_576,
        };
        assert_eq!(awaiting.state(), RebuildAwaiting::Awaiting);
        assert_eq!(RowstoreRebuild::none(HistoryTable::AttributeHistory).state(), RebuildAwaiting::None);
    }

    /// The rebuild names the chunk by its catalogue schema and name, and runs
    /// concurrently so reads of the chunk go on.
    #[test]
    fn a_chunk_is_rebuilt_concurrently_by_its_quoted_name() {
        let chunk = RebuildChunk {
            chunk: HistoryChunk {
                oid: 16_384,
                relation: "_timescaledb_internal._hyper_1_2_chunk".to_owned(),
            },
            schema: "_timescaledb_internal".to_owned(),
            name: "_hyper_1_2_chunk".to_owned(),
            range_start: instant(),
        };
        assert_eq!(
            chunk.rebuild_statement(),
            reindex_table_concurrently(Alias::new("_timescaledb_internal"), Alias::new("_hyper_1_2_chunk"))
        );
    }

    /// A chunk recorded by an earlier rebuild, or by another replica, keeps
    /// its record.
    #[test]
    fn a_rebuilt_chunk_is_recorded_once_by_its_start() {
        let sql = rebuilt_statement(HistoryTable::AttributeHistory, instant())
            .expect("a two-value row")
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"INSERT INTO "rebuilt_history_chunks" ("range_start", "rebuilt_at") VALUES ("#),
            "{sql}"
        );
        assert!(sql.ends_with(r#"ON CONFLICT ("range_start") DO NOTHING"#), "{sql}");
    }
}
