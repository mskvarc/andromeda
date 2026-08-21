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
    pub fn new(pool: &PgPool) -> EntityMapRepository {
        EntityMapRepository { pool: pool.clone() } // clone: `PgPool` is an atomically reference-counted pool handle.
    }

    /// Borrow the pool for the store implementation.
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }
}
