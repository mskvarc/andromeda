//! PostgreSQL advisory-lock capabilities coordinating parallel test processes.

use crate::configuration::DatabaseEndpoint;
use sqlx::{PgPool, Postgres, pool::PoolConnection, postgres::PgPoolOptions};
use std::time::Duration;
use thiserror::Error;

const ISOLATION_LOCK_KEY: i64 = 0x414e_4452_4f4d_4544;

/// Shared lease held by an ordinary prefix-isolated test.
pub struct IsolatedLease {
    connection: Option<PoolConnection<Postgres>>,
}

impl IsolatedLease {
    /// Acquire a cross-process shared advisory lock.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseIsolationError::Sqlx`] when the pool cannot hand out a connection or the `pg_advisory_lock_shared` call fails.
    pub async fn acquire(pool: &PgPool) -> Result<IsolatedLease, DatabaseIsolationError> {
        let mut connection = pool.acquire().await.map_err(DatabaseIsolationError::from_sqlx)?;
        sqlx::query("SELECT pg_advisory_lock_shared($1)")
            .bind(ISOLATION_LOCK_KEY)
            .execute(&mut *connection)
            .await
            .map_err(DatabaseIsolationError::from_sqlx)?;
        Ok(IsolatedLease { connection: Some(connection) })
    }

    /// Release the shared lock on the same PostgreSQL session.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseIsolationError::Sqlx`] when the `pg_advisory_unlock_shared` statement fails, and [`DatabaseIsolationError::LockNotHeld`] when the server reports that this session held no shared lock.
    pub async fn release(mut self) -> Result<(), DatabaseIsolationError> {
        if let Some(mut connection) = self.connection.take() {
            let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock_shared($1)")
                .bind(ISOLATION_LOCK_KEY)
                .fetch_one(&mut *connection)
                .await
                .map_err(DatabaseIsolationError::from_sqlx)?;
            if !released {
                return Err(DatabaseIsolationError::LockNotHeld);
            }
        }
        Ok(())
    }
}

impl Drop for IsolatedLease {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.as_mut() {
            connection.close_on_drop();
        }
    }
}

/// Exclusive lease required by global reset and tenant-lifecycle tests.
pub struct SerializedLease {
    connection: Option<PoolConnection<Postgres>>,
}

impl SerializedLease {
    /// Acquire a cross-process exclusive advisory lock.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseIsolationError::Sqlx`] when the pool cannot hand out a connection or the `pg_advisory_lock` call fails.
    pub async fn acquire(pool: &PgPool) -> Result<SerializedLease, DatabaseIsolationError> {
        let mut connection = pool.acquire().await.map_err(DatabaseIsolationError::from_sqlx)?;
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(ISOLATION_LOCK_KEY)
            .execute(&mut *connection)
            .await
            .map_err(DatabaseIsolationError::from_sqlx)?;
        Ok(SerializedLease { connection: Some(connection) })
    }

    /// Borrow the locked connection for a destructive reset operation.
    pub(crate) fn connection(&mut self) -> Result<&mut PoolConnection<Postgres>, DatabaseIsolationError> {
        self.connection.as_mut().ok_or(DatabaseIsolationError::LockNotHeld)
    }

    /// Release the exclusive lock on the same PostgreSQL session.
    ///
    /// # Errors
    ///
    /// Returns [`DatabaseIsolationError::Sqlx`] when the `pg_advisory_unlock` statement fails, and [`DatabaseIsolationError::LockNotHeld`] when the server reports that this session held no exclusive lock.
    pub async fn release(mut self) -> Result<(), DatabaseIsolationError> {
        if let Some(mut connection) = self.connection.take() {
            let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock($1)")
                .bind(ISOLATION_LOCK_KEY)
                .fetch_one(&mut *connection)
                .await
                .map_err(DatabaseIsolationError::from_sqlx)?;
            if !released {
                return Err(DatabaseIsolationError::LockNotHeld);
            }
        }
        Ok(())
    }
}

impl Drop for SerializedLease {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.as_mut() {
            connection.close_on_drop();
        }
    }
}

/// Connections a harness pool may hold: teardown and persistence probes run
/// one statement at a time, so two connections cover a probe overlapping a
/// cleanup without letting a multi-broker suite crowd the broker's own pools.
const POOL_MAX_CONNECTIONS: u32 = 2;

/// How long an acquire may wait before the harness reports the server as
/// saturated instead of hanging the test.
const POOL_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

/// Open a bounded pool for the configured test database.
///
/// # Errors
///
/// Returns [`DatabaseIsolationError::Sqlx`] when the first connection cannot be established before the pool's acquire timeout elapses.
pub async fn connect_pool(endpoint: &DatabaseEndpoint) -> Result<PgPool, DatabaseIsolationError> {
    PgPoolOptions::new()
        .min_connections(0)
        .max_connections(POOL_MAX_CONNECTIONS)
        .acquire_timeout(POOL_ACQUIRE_TIMEOUT)
        .connect(endpoint.url().as_str())
        .await
        .map_err(DatabaseIsolationError::from_sqlx)
}

/// How long a lazily opened harness connection stays idle before it is closed.
const LAZY_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(5);

/// A bounded pool for `endpoint` that opens no connection until a statement
/// needs one and closes connections soon after they fall idle.
///
/// A multi-broker topology keeps a pool per broker store for its whole test but
/// uses it only around setup and teardown, so holding a connection open in
/// between would take a slot of the server's `max_connections` for nothing.
///
/// # Errors
///
/// Returns [`DatabaseIsolationError::Sqlx`] when the endpoint is not a usable
/// connection URL.
pub fn connect_lazy_pool(endpoint: &DatabaseEndpoint) -> Result<PgPool, DatabaseIsolationError> {
    PgPoolOptions::new()
        .min_connections(0)
        .max_connections(POOL_MAX_CONNECTIONS)
        .acquire_timeout(POOL_ACQUIRE_TIMEOUT)
        .idle_timeout(LAZY_POOL_IDLE_TIMEOUT)
        .connect_lazy(endpoint.url().as_str())
        .map_err(DatabaseIsolationError::from_sqlx)
}

/// Database connection or advisory-lock failure.
#[derive(Debug, Error)]
pub enum DatabaseIsolationError {
    /// `SQLx` could not connect or execute the lock operation.
    ///
    /// Boxed because an `sqlx::Error` is 56 bytes, dominated by a config-file
    /// variant that carries a `PathBuf` and a type-erased source, and this
    /// variant is the whole width of every harness error that reaches the
    /// database: an inline copy widened the `Err` of every environment,
    /// topology, and cleanup operation built on one.
    #[error("test database isolation failed: {0}")]
    Sqlx(#[source] Box<sqlx::Error>),
    /// The capability attempted to release or use a lock it no longer held.
    #[error("test database advisory lock is not held")]
    LockNotHeld,
}

impl DatabaseIsolationError {
    /// Record an `SQLx` failure, moving it behind the variant's box.
    #[must_use]
    pub fn from_sqlx(error: sqlx::Error) -> DatabaseIsolationError {
        DatabaseIsolationError::Sqlx(Box::new(error))
    }
}
