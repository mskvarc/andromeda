//! SQL-only repository for the subscription forwarding mappings.

use sqlx::postgres::PgPool;

/// Repository over `subscription_companions` and `forwarded_subscriptions`.
#[derive(Debug)]
pub struct SubscriptionForwardingRepository {
    /// The tenant connection pool.
    pool: PgPool,
}

impl SubscriptionForwardingRepository {
    /// Create a repository backed by the given tenant pool.
    pub fn new(pool: &PgPool) -> SubscriptionForwardingRepository {
        SubscriptionForwardingRepository { pool: pool.clone() } // clone: `PgPool` is an atomically reference-counted pool handle.
    }

    /// Borrow the pool for the store implementation.
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }
}
