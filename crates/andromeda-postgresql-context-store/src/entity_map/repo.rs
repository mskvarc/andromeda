//! SQL-only repository for Entity Map records.

use sqlx::postgres::PgPool;

/// Repository for persisting and querying Entity Maps.
#[derive(Debug)]
pub struct EntityMapRepository {
    /// The tenant connection pool.
    pool: PgPool,
}

impl EntityMapRepository {
    /// Create a new Entity Map repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: &PgPool) -> EntityMapRepository {
        EntityMapRepository { pool: pool.clone() } // clone: `PgPool` is an atomically reference-counted pool handle.
    }

    /// Borrow the pool for the store implementation.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }
}
