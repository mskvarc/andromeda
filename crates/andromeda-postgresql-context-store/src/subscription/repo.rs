//! Repository for NGSI-LD entity subscriptions.
//!
//! The persistence-port implementation lives beside this repository in the
//! shared subscription-store module.

use crate::error::ContextStoreError;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    uri::{NGSI_LD_URN_NID, Uri},
};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use sqlx::postgres::PgPool;
use std::sync::Arc;
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// PostgreSQL-backed repository for NGSI-LD entity subscriptions.
#[derive(Debug)]
pub struct SubscriptionRepository {
    /// The tenant connection pool.
    pub(crate) pool: PgPool,
    /// The tenant's attribute-name dictionary, which a subscription's
    /// attribute lists are interned into.
    pub(crate) attribute_names: Arc<dyn NameLookup<AttributeName>>,
}

impl SubscriptionRepository {
    /// Generates a new unique URN identifier for an entity subscription.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Internal`] when the generated namespace
    /// specific string is refused by the URN builder.
    pub fn generate_id() -> Result<Uri, ContextStoreError> {
        let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Subscription:{}", Uuid::new_v4()))
            .build()
            .map_err(|e| ContextStoreError::Internal(format!("failed to build subscription URN: {e}")))?;
        Ok(Uri::from(urn))
    }

    /// Creates a new repository backed by the given connection pool and the
    /// tenant's shared attribute-name dictionary.
    #[must_use]
    pub fn new(pool: &PgPool, attribute_names: Arc<dyn NameLookup<AttributeName>>) -> SubscriptionRepository {
        SubscriptionRepository {
            pool: pool.clone(), // clone: Arc ref-count bump
            attribute_names,
        }
    }
}
