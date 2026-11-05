//! The advisory lock that keeps one table to one repack at a time.
//!
//! A run holds the session advisory lock keyed on its table's OID for as
//! long as its objects may be in use. The key is the one the `pg_repack`
//! command line takes for the same table, so the controller and an operator
//! running the command never repack one table at once, and recovery never
//! cleans up after a run that is still going: a table's objects are left
//! behind only while nobody holds its lock.

use crate::error::MaintenanceError;
use andromeda_postgresql_seaquery::{
    advisory_lock::{advisory_unlock_pair, try_advisory_lock_pair},
    statement::Statement,
};
use sea_query::Query;
use sqlx::PgConnection;

/// The first key of every table's lock, the one the command line uses.
const LOCK_CLASS: i32 = 16_185_446;

/// The advisory lock of one table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RepackTableLock {
    /// The table's OID, folded into a signed key as the command line folds
    /// it.
    object: i32,
}

/// Whether a session took the lock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TableLockTaken {
    /// This session holds the lock.
    Taken,
    /// Another session holds it.
    HeldElsewhere,
}

impl RepackTableLock {
    /// The lock of the table `relid`: its OID shifted by -2^31, so every
    /// unsigned OID fits the signed key.
    pub(crate) fn of(relid: u32) -> RepackTableLock {
        let shifted = i64::from(relid) - (1_i64 << 31);
        RepackTableLock {
            object: i32::try_from(shifted).unwrap_or(i32::MAX),
        }
    }

    /// Take the lock for the session without waiting.
    pub(crate) async fn try_take(self, connection: &mut PgConnection) -> Result<TableLockTaken, MaintenanceError> {
        let statement = Query::select().expr(try_advisory_lock_pair(LOCK_CLASS, self.object)).to_owned();
        Ok(if statement.to_query_scalar::<bool>().fetch_one(connection).await? {
            TableLockTaken::Taken
        } else {
            TableLockTaken::HeldElsewhere
        })
    }

    /// Release the lock this session took.
    pub(crate) async fn release(self, connection: &mut PgConnection) -> Result<(), MaintenanceError> {
        let statement = Query::select().expr(advisory_unlock_pair(LOCK_CLASS, self.object)).to_owned();
        statement.to_query_scalar::<bool>().fetch_one(connection).await?;
        Ok(())
    }

    /// Whether another session holds the lock, read by taking it and
    /// releasing it at once.
    pub(crate) async fn probe(self, connection: &mut PgConnection) -> Result<TableLockTaken, MaintenanceError> {
        let taken = self.try_take(&mut *connection).await?;
        if taken == TableLockTaken::Taken {
            self.release(connection).await?;
        }
        Ok(taken)
    }
}

#[cfg(test)]
mod tests {
    use crate::repack::table_lock::RepackTableLock;
    use pretty_assertions::assert_eq;

    /// The command line computes `-2147483648 + oid`.
    #[test]
    fn the_key_folds_the_oid_as_the_command_line_does() {
        assert_eq!(RepackTableLock::of(16_384).object, -2_147_467_264);
        assert_eq!(RepackTableLock::of(0).object, i32::MIN);
        assert_eq!(RepackTableLock::of(u32::MAX).object, i32::MAX);
    }
}
