//! Repository for NGSI-LD context source subscriptions.
//!
//! The `create`, `update`, and `SubscriptionStore` trait impl blocks live in the
//! binary crate, because they depend on `SubscriptionFields` and the
//! `impl_subscription_store!` macro which reference binary-side service types.

use sqlx::postgres::PgPool;

/// Repository for persisting and querying context source subscriptions.
#[derive(Debug)]
pub struct CsourceSubscriptionRepository {
    /// The tenant connection pool.
    pub pool: PgPool,
}

impl CsourceSubscriptionRepository {
    /// Create a new context source subscription repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> CsourceSubscriptionRepository {
        CsourceSubscriptionRepository { pool: pool.clone() } // clone: Arc ref-count bump
    }
}
