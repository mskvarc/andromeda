//! Returning the rowstore index pages of converted history chunks to the file
//! system.
//!
//! A conversion that had to delete its rowstore rows rather than truncate the
//! relation, and a recompression of a partial chunk, leave the chunk's rowstore
//! indexes at the size they reached. Vacuum empties the heap and truncates it,
//! since every row is dead, but returns no index page, and a converted chunk
//! receives too few rows to reuse them: on a chunk of 1.68 million rows the
//! indexes kept 475 MB beside 73 MB of columnstore. Each sweep rebuilds the
//! indexes of every converted chunk whose heap vacuum has emptied and whose
//! indexes still exceed [`RECLAIM_FLOOR_BYTES`].
//!
//! The rebuild is `REINDEX TABLE` inside a transaction of its own: over an
//! empty heap it builds empty indexes in milliseconds, and as a transaction it
//! either completes or leaves the indexes as they were, with none of the
//! invalid indexes an interrupted `REINDEX CONCURRENTLY` leaves behind. It
//! takes a `SHARE` lock on the chunk and an `ACCESS EXCLUSIVE` lock on each of
//! its indexes, so it waits at most [`RECLAIM_LOCK_TIMEOUT`] for them and is
//! retried on the next sweep otherwise: a request queued longer would hold up
//! every read of the chunk behind it. A chunk whose heap still holds a page,
//! because vacuum has not yet reached it or a later write left it partial, is
//! left for a later sweep.

use crate::{error::TemporalStoreError, hypertable_policy::HypertableName, repo::TemporalRepository};
use andromeda_postgresql_pool::{schema_change_lock::is_lock_not_available, transaction_rollback::is_transaction_rolled_back};
use andromeda_postgresql_seaquery::{
    columnstore_chunk::{EmptiedChunkName, EmptiedChunkSchema, emptied_rowstore_indexes_statement},
    command::Command,
    maintenance::reindex_table,
    setting::{Setting, SettingScope, set_config},
    statement::Statement,
};
use sea_query::{Alias, Iden, Query, SelectStatement};
use sqlx::Row;

/// The hypertable whose converted chunks are reclaimed.
const RECLAIMED_HYPERTABLE: HypertableName = HypertableName::AttributeHistory;

/// The index bytes past which an emptied chunk's indexes are rebuilt: one
/// mebibyte, about a hundred times what the rebuild leaves.
pub const RECLAIM_FLOOR_BYTES: i64 = 1_048_576;

/// How long a rebuild waits for its locks, in the form `lock_timeout` takes.
pub const RECLAIM_LOCK_TIMEOUT: &str = "200ms";

/// What one reclaim pass did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RowstoreReclaim {
    /// Chunks whose indexes were rebuilt.
    pub rebuilt: usize,
    /// Chunks whose rebuild waited out its lock timeout or was rolled back as
    /// a deadlock victim; the next sweep tries again.
    pub deferred: usize,
}

impl TemporalRepository {
    /// Rebuild the rowstore indexes of every converted history chunk whose
    /// heap is empty and whose indexes exceed [`RECLAIM_FLOOR_BYTES`].
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the catalogue cannot be
    /// read or a rebuild fails for a reason other than its lock wait.
    pub async fn reclaim_converted_rowstores(&self) -> Result<RowstoreReclaim, TemporalStoreError> {
        let rows = emptied_rowstore_indexes_statement(RECLAIMED_HYPERTABLE.into(), RECLAIM_FLOOR_BYTES)
            .to_query()
            .fetch_all(&self.pool)
            .await?;
        let mut reclaim = RowstoreReclaim::default();
        for row in rows {
            let schema: String = row.try_get(EmptiedChunkSchema.unquoted())?;
            let name: String = row.try_get(EmptiedChunkName.unquoted())?;
            let mut transaction = self.pool.begin().await?;
            let rebuilt = async {
                rebuild_lock_timeout().to_query().execute(&mut *transaction).await?;
                reindex_table(Alias::new(schema), Alias::new(name)).to_query().execute(&mut *transaction).await
            }
            .await;
            match rebuilt {
                Ok(_) => {
                    transaction.commit().await?;
                    reclaim.rebuilt += 1;
                }
                Err(error) if is_lock_not_available(&error) || is_transaction_rolled_back(&error) => {
                    transaction.rollback().await?;
                    reclaim.deferred += 1;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(reclaim)
    }
}

/// `SELECT set_config('lock_timeout', RECLAIM_LOCK_TIMEOUT, true)`.
fn rebuild_lock_timeout() -> SelectStatement {
    Query::select()
        .expr(set_config(Setting::LockTimeout, RECLAIM_LOCK_TIMEOUT, SettingScope::Transaction))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::rowstore_reclaim::rebuild_lock_timeout;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn a_rebuild_waits_a_fifth_of_a_second_for_its_locks_within_its_transaction() {
        assert_eq!(
            rebuild_lock_timeout().to_string(PostgresQueryBuilder),
            "SELECT set_config('lock_timeout', '200ms', TRUE)"
        );
    }
}
