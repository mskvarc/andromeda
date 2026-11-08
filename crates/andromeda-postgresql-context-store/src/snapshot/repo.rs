//! SQL-only repository for snapshot records.
//!
//! This module provides the struct and constructor; the `SnapshotStore` trait
//! impl is in a separate module.

use sqlx::postgres::PgPool;

/// Repository for persisting and querying snapshot records.
#[derive(Debug)]
pub struct SnapshotRepository {
    /// The tenant connection pool.
    pool: PgPool,
}

impl SnapshotRepository {
    /// Create a new snapshot repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: &PgPool) -> SnapshotRepository {
        SnapshotRepository { pool: pool.clone() }
    }

    /// Borrow the pool for trait impls defined outside this crate.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }
}
