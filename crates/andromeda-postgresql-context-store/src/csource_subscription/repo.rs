//! Repository for NGSI-LD context source subscriptions.
//!
//! The persistence-port implementation lives beside this repository in the
//! shared subscription-store module.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_pool::pool_share::PoolShare;
use sqlx::postgres::PgPool;
use std::sync::Arc;

/// Repository for persisting and querying context source subscriptions.
#[derive(Debug)]
pub struct CsourceSubscriptionRepository {
    /// The tenant connection pool, which every read and every write that
    /// leaves the context source change clock alone runs on.
    pub(crate) pool: PgPool,
    /// The tenant's clock-writer share of the pool, which a client create or
    /// replacement begins its transaction on, since both advance the context
    /// source change clock.
    pub(crate) clock_writers: PoolShare,
    /// The tenant's attribute-name dictionary, which a subscription's
    /// attribute lists are interned into.
    pub(crate) attribute_names: Arc<dyn NameLookup<AttributeName>>,
}

impl CsourceSubscriptionRepository {
    /// Create a new context source subscription repository backed by the given
    /// connection pool, the tenant's clock-writer share of it and the tenant's
    /// shared attribute-name dictionary.
    #[must_use]
    pub fn new(pool: &PgPool, clock_writers: PoolShare, attribute_names: Arc<dyn NameLookup<AttributeName>>) -> CsourceSubscriptionRepository {
        CsourceSubscriptionRepository {
            pool: pool.clone(),
            clock_writers,
            attribute_names,
        }
    }
}
