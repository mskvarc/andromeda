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
    pub async fn acquire(pool: &PgPool) -> Result<IsolatedLease, DatabaseIsolationError> {
        let mut connection = pool.acquire().await.map_err(DatabaseIsolationError::Sqlx)?;
        sqlx::query("SELECT pg_advisory_lock_shared($1)")
            .bind(ISOLATION_LOCK_KEY)
            .execute(&mut *connection)
            .await
            .map_err(DatabaseIsolationError::Sqlx)?;
        Ok(IsolatedLease { connection: Some(connection) })
    }

    /// Release the shared lock on the same PostgreSQL session.
    pub async fn release(mut self) -> Result<(), DatabaseIsolationError> {
        if let Some(mut connection) = self.connection.take() {
            let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock_shared($1)")
                .bind(ISOLATION_LOCK_KEY)
                .fetch_one(&mut *connection)
                .await
                .map_err(DatabaseIsolationError::Sqlx)?;
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
    pub async fn acquire(pool: &PgPool) -> Result<SerializedLease, DatabaseIsolationError> {
        let mut connection = pool.acquire().await.map_err(DatabaseIsolationError::Sqlx)?;
        sqlx::query("SELECT pg_advisory_lock($1)")
            .bind(ISOLATION_LOCK_KEY)
            .execute(&mut *connection)
            .await
            .map_err(DatabaseIsolationError::Sqlx)?;
        Ok(SerializedLease { connection: Some(connection) })
    }

    /// Borrow the locked connection for a destructive reset operation.
    pub(crate) fn connection(&mut self) -> Result<&mut PoolConnection<Postgres>, DatabaseIsolationError> {
        self.connection.as_mut().ok_or(DatabaseIsolationError::LockNotHeld)
    }

    /// Release the exclusive lock on the same PostgreSQL session.
    pub async fn release(mut self) -> Result<(), DatabaseIsolationError> {
        if let Some(mut connection) = self.connection.take() {
            let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock($1)")
                .bind(ISOLATION_LOCK_KEY)
                .fetch_one(&mut *connection)
                .await
                .map_err(DatabaseIsolationError::Sqlx)?;
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
pub async fn connect_pool(endpoint: &DatabaseEndpoint) -> Result<PgPool, DatabaseIsolationError> {
    PgPoolOptions::new()
        .min_connections(0)
        .max_connections(POOL_MAX_CONNECTIONS)
        .acquire_timeout(POOL_ACQUIRE_TIMEOUT)
        .connect(endpoint.url().as_str())
        .await
        .map_err(DatabaseIsolationError::Sqlx)
}

/// Database connection or advisory-lock failure.
#[derive(Debug, Error)]
pub enum DatabaseIsolationError {
    /// `SQLx` could not connect or execute the lock operation.
    #[error("test database isolation failed: {0}")]
    Sqlx(#[source] sqlx::Error),
    /// The capability attempted to release or use a lock it no longer held.
    #[error("test database advisory lock is not held")]
    LockNotHeld,
}
