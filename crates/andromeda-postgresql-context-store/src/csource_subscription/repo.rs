//! Repository for NGSI-LD context source subscriptions.
//!
//! The persistence-port implementation lives beside this repository in the
//! shared subscription-store module.

use sqlx::postgres::PgPool;

/// Repository for persisting and querying context source subscriptions.
#[derive(Debug)]
pub struct CsourceSubscriptionRepository {
    /// The tenant connection pool.
    pub(crate) pool: PgPool,
}

impl CsourceSubscriptionRepository {
    /// Create a new context source subscription repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: &PgPool) -> CsourceSubscriptionRepository {
        CsourceSubscriptionRepository { pool: pool.clone() } // clone: Arc ref-count bump
    }
}
