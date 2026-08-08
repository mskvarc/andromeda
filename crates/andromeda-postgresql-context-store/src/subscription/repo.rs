//! Repository for NGSI-LD entity subscriptions.
//!
//! The `create`, `update`, and `SubscriptionStore` trait impl blocks live in the
//! binary crate, because they depend on `SubscriptionFields` and the
//! `impl_subscription_store!` macro which reference binary-side service types.

use crate::error::ContextStoreError;
use andromeda_ngsi_ld_lexicon::uri::NGSI_LD_URN_NID;
use sqlx::postgres::PgPool;
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// PostgreSQL-backed repository for NGSI-LD entity subscriptions.
#[derive(Debug)]
pub struct SubscriptionRepository {
    /// The tenant connection pool.
    pub pool: PgPool,
}

impl SubscriptionRepository {
    /// Generates a new unique URN identifier for an entity subscription.
    pub fn generate_id() -> Result<String, ContextStoreError> {
        let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Subscription:{}", Uuid::new_v4()))
            .build()
            .map_err(|e| ContextStoreError::Internal(format!("failed to build subscription URN: {e}")))?;
        Ok(urn.to_string())
    }

    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> SubscriptionRepository {
        SubscriptionRepository { pool: pool.clone() } // clone: Arc ref-count bump
    }
}
