//! Persistence of the tenant-scoped subscription forwarding mappings.

/// The repository struct.
pub mod repo;
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
