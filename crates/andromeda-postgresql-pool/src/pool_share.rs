//! The connections of a tenant pool one class of requests may hold.
//!
//! A holder of a share takes a permit before its connection, so no more
//! holders hold connections at once than the share admits, and the rest of the
//! pool stays free of them. A tenant pool has two shares.
//!
//! - The read share admits what the [`WriteReserve`] leaves to read requests.
//!   A write takes its connection from the pool directly and so always finds
//!   the reserve. Reads that a write makes are the write's, and take the pool
//!   directly too.
//! - The reserve share admits what the [`WriteReserve`] keeps, for the writes
//!   that queue on one row lock, so however many of them wait on the lock they
//!   hold no more of the pool than the reserve.
//!
//! Waiting for the permit and for the connection share one acquire deadline,
//! the pool's own, so a saturated share is shed as the same
//! [`sqlx::Error::PoolTimedOut`] a saturated pool is. A share admits at least
//! one holder, so no request waits on a share that can never admit it.
//!
//! A read runs its statement on the connection it holds as the statement's
//! [`StatementPlanning`] asks, so every store plans a gated or value-dependent
//! statement the same way.

use andromeda_configuration::schema::database::write_reserve::WriteReserve;
use andromeda_postgresql_seaquery::{committing_statement::CommittingStatement, statement::Statement, statement_planning::StatementPlanning};
use derive_more::{Deref, DerefMut};
use sea_query::SelectStatement;
use sqlx::{Decode, PgPool, Postgres, Transaction, Type, pool::PoolConnection, postgres::PgRow};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::timeout,
};

/// Which connections of a tenant pool a statement may run on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolLane {
    /// Any connection, for a write and every read a write makes.
    Whole,
    /// Only the connections the write reserve leaves to read requests.
    Reads,
}

/// One class of requests' share of a tenant pool.
///
/// Cloning shares the permits, so every store of a tenant draws one class of
/// requests from one share.
#[derive(Clone, Debug)]
pub struct PoolShare {
    /// The tenant pool the connections come from.
    pool: PgPool,
    /// One permit per connection the share's holders may hold at once.
    permits: Arc<Semaphore>,
    /// How long a holder waits for its permit and its connection together,
    /// the pool's own acquire deadline.
    acquire_timeout: Duration,
}

/// A connection a holder of a share holds, with the permit it was admitted by.
///
/// The connection is declared first, so it is back in the pool before the
/// permit admits the next holder.
#[derive(Debug, Deref, DerefMut)]
pub struct SharedConnection {
    /// The pooled connection.
    #[deref]
    #[deref_mut]
    connection: PoolConnection<Postgres>,
    /// The permit, released when the connection has been returned.
    _permit: OwnedSemaphorePermit,
}

/// A transaction a holder of a share runs, with the permit it was admitted by.
///
/// Statements run on it as on the connection it holds. The transaction is
/// declared first, so its connection is back in the pool before the permit
/// admits the next holder. Dropped without a commit, it rolls back as any
/// transaction does.
#[derive(Debug, Deref, DerefMut)]
pub struct SharedTransaction {
    /// The transaction, on a connection of the share's pool.
    #[deref(forward)]
    #[deref_mut(forward)]
    transaction: Transaction<'static, Postgres>,
    /// The permit, released once the transaction has ended.
    _permit: OwnedSemaphorePermit,
}

impl PoolShare {
    /// The share of `pool` that `reserve` leaves to read requests.
    #[must_use]
    pub fn of_reads(pool: &PgPool, reserve: WriteReserve) -> PoolShare {
        PoolShare::admitting(pool, reserve.read_connections(pool.options().get_max_connections()))
    }

    /// The share of `pool` that `reserve` keeps from read requests, for writes
    /// that queue on one row lock.
    #[must_use]
    pub fn of_reserve(pool: &PgPool, reserve: WriteReserve) -> PoolShare {
        PoolShare::admitting(pool, reserve.reserved_connections(pool.options().get_max_connections()))
    }

    /// The share of `pool` admitting `connections` holders, and at least one.
    fn admitting(pool: &PgPool, connections: u32) -> PoolShare {
        PoolShare {
            pool: pool.clone(),
            permits: Arc::new(Semaphore::new(permit_count(connections))),
            acquire_timeout: pool.options().get_acquire_timeout(),
        }
    }

    /// A connection for a holder, once the share admits one.
    ///
    /// # Errors
    ///
    /// Returns [`sqlx::Error::PoolTimedOut`] when no permit and connection are
    /// free within the acquire deadline, and the pool's error when a
    /// connection cannot be opened.
    pub async fn acquire(&self) -> Result<SharedConnection, sqlx::Error> {
        timeout(self.acquire_timeout, self.admit_connection())
            .await
            .unwrap_or(Err(sqlx::Error::PoolTimedOut))
    }

    /// A transaction for a holder, begun once the share admits one.
    ///
    /// # Errors
    ///
    /// Returns [`sqlx::Error::PoolTimedOut`] when no permit and connection are
    /// free within the acquire deadline, and the pool's error when a
    /// connection cannot be opened or the transaction cannot begin.
    pub async fn begin(&self) -> Result<SharedTransaction, sqlx::Error> {
        timeout(self.acquire_timeout, self.admit_transaction())
            .await
            .unwrap_or(Err(sqlx::Error::PoolTimedOut))
    }

    /// Wait for a permit, then for a connection.
    async fn admit_connection(&self) -> Result<SharedConnection, sqlx::Error> {
        let permit = self.permit().await?;
        let connection = self.pool.acquire().await?;
        Ok(SharedConnection { connection, _permit: permit })
    }

    /// Wait for a permit, then begin a transaction on a connection.
    async fn admit_transaction(&self) -> Result<SharedTransaction, sqlx::Error> {
        let permit = self.permit().await?;
        let transaction = self.pool.begin().await?;
        Ok(SharedTransaction { transaction, _permit: permit })
    }

    /// Wait for a permit of the share.
    async fn permit(&self) -> Result<OwnedSemaphorePermit, sqlx::Error> {
        Arc::clone(&self.permits).acquire_owned().await.map_err(|_closed| sqlx::Error::PoolClosed)
    }
}

impl SharedConnection {
    /// Run `statement` as `planning` asks, alone or in a transaction of its
    /// own that sets how it is planned and run.
    ///
    /// # Errors
    ///
    /// Returns the driver's error when the statement fails, or the
    /// transaction it runs in cannot begin or commit.
    pub async fn fetch_planned(&mut self, statement: &SelectStatement, planning: StatementPlanning) -> Result<Vec<PgRow>, sqlx::Error> {
        let query = statement.to_query();
        let Some(start) = planning.transaction() else {
            return query.fetch_all(&mut *self.connection).await;
        };
        let mut transaction = start.begin_on(&mut self.connection).await?;
        let rows = query.fetch_all(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(rows)
    }
}

impl SharedTransaction {
    /// Commit the transaction, then release the permit.
    ///
    /// # Errors
    ///
    /// Returns the driver's error when the commit fails.
    pub async fn commit(self) -> Result<(), sqlx::Error> {
        self.transaction.commit().await
    }

    /// Roll the transaction back, then release the permit.
    ///
    /// # Errors
    ///
    /// Returns the driver's error when the rollback fails.
    pub async fn rollback(self) -> Result<(), sqlx::Error> {
        self.transaction.rollback().await
    }

    /// Run `last` and commit in the one round trip that carries both, then
    /// release the permit, returning the first column of the first row `last`
    /// returns.
    ///
    /// # Errors
    ///
    /// Returns the driver's error when `last` or the commit fails, and
    /// [`sqlx::Error::RowNotFound`] when `last` returns no row.
    pub async fn commit_fetching<O>(self, last: &CommittingStatement) -> Result<O, sqlx::Error>
    where
        O: for<'row> Decode<'row, Postgres> + Type<Postgres>,
    {
        let SharedTransaction { mut transaction, _permit } = self;
        let value = last.fetch_ending(&mut transaction).await?;
        close_committed(transaction).await;
        Ok(value)
    }

    /// Run `last` and commit in the one round trip that carries both, then
    /// release the permit.
    ///
    /// # Errors
    ///
    /// Returns the driver's error when `last` or the commit fails.
    pub async fn commit_executing(self, last: &CommittingStatement) -> Result<(), sqlx::Error> {
        let SharedTransaction { mut transaction, _permit } = self;
        last.execute_ending(&mut transaction).await?;
        close_committed(transaction).await;
        Ok(())
    }
}

/// Close the empty transaction a [`CommittingStatement`] leaves open once it
/// has committed the write before it.
///
/// The write is durable by then, so a close that fails changes nothing about
/// it. Only the connection can fail it, and the readiness probe the pool runs
/// on a returned connection then finds it broken and closes it.
async fn close_committed(transaction: Transaction<'static, Postgres>) {
    if let Err(error) = transaction.commit().await {
        tracing::warn!(%error, "the empty transaction after a committed write could not be closed");
    }
}

/// The permits of a share of `connections`, at least one and at most as many
/// as a semaphore can count.
fn permit_count(connections: u32) -> usize {
    usize::try_from(connections).map_or(Semaphore::MAX_PERMITS, |count| count.clamp(1, Semaphore::MAX_PERMITS))
}

#[cfg(test)]
mod tests {
    use crate::{pool_options::configure_pool_options, pool_share::PoolShare};
    use andromeda_configuration::schema::database::write_reserve::WriteReserve;
    use pretty_assertions::assert_eq;
    use std::{sync::Arc, time::Duration};

    fn lazy_pool(max_connections: u32) -> sqlx::PgPool {
        configure_pool_options(max_connections, Duration::from_millis(50))
            .connect_lazy("postgres://unreachable.invalid/none")
            .expect("a lazy pool")
    }

    fn reserve(percent: u8) -> WriteReserve {
        WriteReserve::try_from(percent).expect("a reserve below 100 percent")
    }

    #[tokio::test]
    async fn a_read_share_admits_the_connections_the_reserve_leaves_to_reads() {
        assert_eq!(PoolShare::of_reads(&lazy_pool(32), WriteReserve::DEFAULT).permits.available_permits(), 24);
        assert_eq!(PoolShare::of_reads(&lazy_pool(2), WriteReserve::DEFAULT).permits.available_permits(), 2);
    }

    #[tokio::test]
    async fn a_reserve_share_admits_the_connections_the_reserve_keeps() {
        assert_eq!(PoolShare::of_reserve(&lazy_pool(32), WriteReserve::DEFAULT).permits.available_permits(), 8);
        assert_eq!(PoolShare::of_reserve(&lazy_pool(4), WriteReserve::DEFAULT).permits.available_permits(), 1);
    }

    #[tokio::test]
    async fn a_share_of_no_connections_still_admits_one_holder() {
        assert_eq!(PoolShare::of_reserve(&lazy_pool(2), WriteReserve::DEFAULT).permits.available_permits(), 1);
        assert_eq!(PoolShare::of_reserve(&lazy_pool(32), reserve(0)).permits.available_permits(), 1);
    }

    #[tokio::test]
    async fn a_clone_draws_on_the_same_permits() {
        let share = PoolShare::of_reads(&lazy_pool(4), WriteReserve::DEFAULT);
        let clone = share.clone();
        let held = Arc::clone(&share.permits).try_acquire_owned().expect("a free permit");
        assert_eq!(clone.permits.available_permits(), 2);
        drop(held);
        assert_eq!(clone.permits.available_permits(), 3);
    }

    #[tokio::test]
    async fn a_connection_waiting_past_the_acquire_deadline_is_shed_as_a_saturated_pool() {
        let share = PoolShare::of_reads(&lazy_pool(1), WriteReserve::DEFAULT);
        let held = Arc::clone(&share.permits).try_acquire_owned().expect("the one permit");
        let refused = share.acquire().await;
        assert!(matches!(refused, Err(sqlx::Error::PoolTimedOut)), "{refused:?}");
        drop(held);
        assert_eq!(share.permits.available_permits(), 1);
    }

    #[tokio::test]
    async fn a_transaction_waiting_past_the_acquire_deadline_is_shed_as_a_saturated_pool() {
        let share = PoolShare::of_reserve(&lazy_pool(4), WriteReserve::DEFAULT);
        let held = Arc::clone(&share.permits).try_acquire_owned().expect("the one permit");
        let refused = share.begin().await;
        assert!(matches!(refused, Err(sqlx::Error::PoolTimedOut)), "{refused:?}");
        drop(held);
        assert_eq!(share.permits.available_permits(), 1);
    }
}
