//! Persistence of the tenant-scoped subscription forwarding mappings.

use crate::store_fault::{StoreFault, persistence_failure};
use andromeda_subscription_forwarding::error::SubscriptionForwardingError;

/// The repository struct.
pub mod repo;
pub mod store;

/// Erase an adapter failure into the forwarding policy's persistence category.
///
/// Classify an adapter failure at the port boundary: backpressure keeps the
/// port's retryable category and every fault becomes its persistence failure.
#[track_caller]
fn persistence_error(error: impl StoreFault) -> SubscriptionForwardingError {
    SubscriptionForwardingError::from(persistence_failure(&error))
}

#[cfg(test)]
mod tests {
    use crate::subscription_forwarding::persistence_error;
    use andromeda_subscription_forwarding::error::SubscriptionForwardingError;

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        assert!(matches!(
            persistence_error(sqlx::Error::PoolTimedOut),
            SubscriptionForwardingError::StoreSaturated
        ));
        assert!(matches!(
            persistence_error(sqlx::Error::RowNotFound),
            SubscriptionForwardingError::Persistence { .. }
        ));
    }
}
