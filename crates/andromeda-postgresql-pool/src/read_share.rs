//! The connections of a tenant pool read requests may hold.
//!
//! A read request takes a permit before its connection, so no more read
//! requests hold connections at once than the [`WriteReserve`] leaves them; a
//! write takes its connection from the pool directly and so always finds the
//! reserve. Reads that a write makes are the write's, and take the pool
//! directly too. Waiting for the permit and for the connection share one
//! acquire deadline, the pool's own, so a saturated share is shed as the same
//! [`sqlx::Error::PoolTimedOut`] a saturated pool is.
//!
//! A read runs its statement on the connection it holds as the statement's
//! [`StatementPlanning`] asks, so every store plans a gated or value-dependent
//! statement the same way.

use andromeda_configuration::schema::write_reserve::WriteReserve;
use andromeda_postgresql_seaquery::{statement::Statement, statement_planning::StatementPlanning};
use derive_more::{Deref, DerefMut};
use sea_query::SelectStatement;
use sqlx::{PgPool, Postgres, pool::PoolConnection, postgres::PgRow};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{OwnedSemaphorePermit, Semaphore},
    time::timeout,
};

/// Which connections of a tenant pool a statement may run on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolLane {
    /// Any connection: a write, and every read a write makes.
    Whole,
    /// Only the connections the write reserve leaves to read requests.
    Reads,
}

/// The read requests' share of one tenant pool.
///
/// Cloning shares the permits, so every store of a tenant draws its read
/// requests from one share.
#[derive(Clone, Debug)]
pub struct ReadShare {
    /// The tenant pool the connections come from.
    pool: PgPool,
    /// One permit per connection read requests may hold at once.
    permits: Arc<Semaphore>,
    /// How long a read request waits for its permit and its connection
    /// together: the pool's own acquire deadline.
    acquire_timeout: Duration,
}

/// A connection a read request holds, with the permit it was admitted by.
///
/// The connection is declared first, so it is back in the pool before the
/// permit admits the next read request.
#[derive(Debug, Deref, DerefMut)]
pub struct ReadConnection {
    /// The pooled connection.
    #[deref]
    #[deref_mut]
    connection: PoolConnection<Postgres>,
    /// The permit, released when the connection has been returned.
    _permit: OwnedSemaphorePermit,
}

impl ReadShare {
    /// The share of `pool` that leaves `reserve` of its connections to writes.
    #[must_use]
    pub fn of(pool: &PgPool, reserve: WriteReserve) -> ReadShare {
        let options = pool.options();
        ReadShare {
            pool: pool.clone(), // clone: `PgPool` shares its inner pool by reference count.
            permits: Arc::new(Semaphore::new(permit_count(reserve.read_connections(options.get_max_connections())))),
            acquire_timeout: options.get_acquire_timeout(),
        }
    }

    /// A connection for a read request, once the share admits one.
    ///
    /// # Errors
    ///
    /// Returns [`sqlx::Error::PoolTimedOut`] when no permit and connection are
    /// free within the acquire deadline, and the pool's error when a
    /// connection cannot be opened.
    pub async fn acquire(&self) -> Result<ReadConnection, sqlx::Error> {
        timeout(self.acquire_timeout, self.admit()).await.unwrap_or(Err(sqlx::Error::PoolTimedOut))
    }

    /// Wait for a permit, then for a connection.
    async fn admit(&self) -> Result<ReadConnection, sqlx::Error> {
        let permit = Arc::clone(&self.permits).acquire_owned().await.map_err(|_closed| sqlx::Error::PoolClosed)?;
        let connection = self.pool.acquire().await?;
        Ok(ReadConnection { connection, _permit: permit })
    }
}

impl ReadConnection {
    /// Run `statement` as `planning` asks: alone, or in a transaction of its
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

/// The permits of a share of `connections`, at most as many as a semaphore
/// can count.
fn permit_count(connections: u32) -> usize {
    usize::try_from(connections).map_or(Semaphore::MAX_PERMITS, |count| count.min(Semaphore::MAX_PERMITS))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{pool_options::configure_pool_options, read_share::ReadShare};
    use andromeda_configuration::schema::write_reserve::WriteReserve;
    use pretty_assertions::assert_eq;
    use std::{sync::Arc, time::Duration};

    fn lazy_pool(max_connections: u32) -> sqlx::PgPool {
        configure_pool_options(max_connections, Duration::from_millis(50))
            .connect_lazy("postgres://unreachable.invalid/none")
            .expect("a lazy pool")
    }

    #[tokio::test]
    async fn a_share_admits_the_connections_the_reserve_leaves_to_reads() {
        assert_eq!(ReadShare::of(&lazy_pool(32), WriteReserve::DEFAULT).permits.available_permits(), 24);
        assert_eq!(ReadShare::of(&lazy_pool(2), WriteReserve::DEFAULT).permits.available_permits(), 2);
    }

    #[tokio::test]
    async fn a_clone_draws_on_the_same_permits() {
        let share = ReadShare::of(&lazy_pool(4), WriteReserve::DEFAULT);
        let clone = share.clone();
        let held = Arc::clone(&share.permits).try_acquire_owned().expect("a free permit");
        assert_eq!(clone.permits.available_permits(), 2);
        drop(held);
        assert_eq!(clone.permits.available_permits(), 3);
    }

    #[tokio::test]
    async fn a_read_waiting_past_the_acquire_deadline_is_shed_as_a_saturated_pool() {
        let share = ReadShare::of(&lazy_pool(1), WriteReserve::DEFAULT);
        let held = Arc::clone(&share.permits).try_acquire_owned().expect("the one permit");
        let refused = share.acquire().await;
        assert!(matches!(refused, Err(sqlx::Error::PoolTimedOut)), "{refused:?}");
        drop(held);
        assert_eq!(share.permits.available_permits(), 1);
    }
}
