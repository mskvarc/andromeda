//! SQL-only repository for posted queries.

use sqlx::postgres::PgPool;

/// Repository for holding and reading posted queries.
#[derive(Debug)]
pub struct PostedQueryRepository {
    /// The tenant connection pool.
    pool: PgPool,
}

impl PostedQueryRepository {
    /// Create a posted-query repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: &PgPool) -> PostedQueryRepository {
        PostedQueryRepository { pool: pool.clone() }
    }

    /// Borrow the pool for the store implementation.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }
}
