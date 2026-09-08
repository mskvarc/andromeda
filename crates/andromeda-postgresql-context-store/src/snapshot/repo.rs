//! SQL-only repository for snapshot records.
//!
//! The `SnapshotStore` trait impl lives in the binary crate, because the trait
//! and its associated types are defined in the binary's service layer. This
//! module provides the struct and constructor.

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
        SnapshotRepository { pool: pool.clone() } // clone: Arc ref-count bump
    }

    /// Borrow the pool for trait impls defined outside this crate.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }
}
