//! Persistence of the tenant-scoped subscription forwarding mappings.

use andromeda_subscription_forwarding::error::SubscriptionForwardingError;
use std::fmt::Display;

/// The repository struct.
pub mod repo;
pub mod store;

/// Erase an adapter failure into the forwarding policy's persistence category.
fn persistence_error(error: impl Display) -> SubscriptionForwardingError {
    SubscriptionForwardingError::persistence(error.to_string())
}
