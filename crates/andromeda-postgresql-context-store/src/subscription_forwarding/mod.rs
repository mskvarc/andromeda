//! Persistence of the tenant-scoped subscription forwarding mappings.

use andromeda_postgresql_pool::is_pool_saturated;
use andromeda_subscription_forwarding::error::SubscriptionForwardingError;
use std::{any::Any, fmt::Display};

/// The repository struct.
pub mod repo;
pub mod store;

/// Erase an adapter failure into the forwarding policy's persistence category.
///
/// A `sqlx` pool-acquire timeout is backpressure, not a fault, so it becomes
/// [`SubscriptionForwardingError::StoreSaturated`] (a 503); every other failure
/// stays a persistence fault. The concrete [`sqlx::Error`] is recovered by
/// downcast because callers hand it in already erased behind `impl Display`.
fn persistence_error(error: impl Display + Any) -> SubscriptionForwardingError {
    match (&error as &dyn Any).downcast_ref::<sqlx::Error>() {
        Some(sqlx_error) if is_pool_saturated(sqlx_error) => SubscriptionForwardingError::StoreSaturated,
        Some(_) | None => SubscriptionForwardingError::persistence(error.to_string()),
    }
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
