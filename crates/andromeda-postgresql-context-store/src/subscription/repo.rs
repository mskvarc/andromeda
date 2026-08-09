//! Repository for NGSI-LD entity subscriptions.
//!
//! The persistence-port implementation lives beside this repository in the
//! shared subscription-store module.

use crate::error::ContextStoreError;
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use sqlx::postgres::PgPool;
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// PostgreSQL-backed repository for NGSI-LD entity subscriptions.
#[derive(Debug)]
pub struct SubscriptionRepository {
    /// The tenant connection pool.
    pub(crate) pool: PgPool,
}

impl SubscriptionRepository {
    /// Generates a new unique URN identifier for an entity subscription.
    pub fn generate_id() -> Result<Uri, ContextStoreError> {
        let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Subscription:{}", Uuid::new_v4()))
            .build()
            .map_err(|e| ContextStoreError::Internal(format!("failed to build subscription URN: {e}")))?;
        Ok(Uri::from(urn))
    }

    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> SubscriptionRepository {
        SubscriptionRepository { pool: pool.clone() } // clone: Arc ref-count bump
    }
}
