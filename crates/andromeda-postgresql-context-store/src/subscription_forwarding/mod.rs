//! Persistence of the tenant-scoped subscription forwarding mappings.

/// The statements over the claims of copies whose source has not answered yet.
mod claim_statements;
/// The repository struct.
pub mod repo;
/// The statements over the two mapping tables, each resolving a subscription's
/// surrogate key from its URI.
mod statements;
pub mod store;

#[cfg(test)]
mod tests {
    use crate::store_fault::persistence_error;
    use andromeda_subscription_forwarding::error::SubscriptionForwardingError;

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        assert!(matches!(
            persistence_error::<SubscriptionForwardingError>(sqlx::Error::PoolTimedOut),
            SubscriptionForwardingError::StoreSaturated
        ));
        assert!(matches!(
            persistence_error::<SubscriptionForwardingError>(sqlx::Error::RowNotFound),
            SubscriptionForwardingError::Persistence { .. }
        ));
    }
}
