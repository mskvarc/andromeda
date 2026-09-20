//! How an adapter failure surfaces at the subscription policy boundary.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_pool::is_pool_saturated;
use andromeda_subscription::error::SubscriptionError;
use std::{any::Any, fmt::Display};

/// Flatten an adapter failure at the policy boundary.
///
/// This is the one boundary where an adapter failure is erased into the policy's
/// persistence category, so it is also where a saturated pool is recognised: a
/// `sqlx` pool-acquire timeout arriving here becomes
/// [`SubscriptionError::StoreSaturated`] (a 503), while every other failure —
/// including a non-`sqlx` decode fault — stays a persistence fault. The concrete
/// [`sqlx::Error`] is recovered by downcast because callers hand it in already
/// erased behind `impl Display`.
pub fn persistence_error(error: impl Display + Any) -> SubscriptionError {
    match (&error as &dyn Any).downcast_ref::<sqlx::Error>() {
        Some(sqlx_error) if is_pool_saturated(sqlx_error) => SubscriptionError::StoreSaturated,
        Some(_) | None => SubscriptionError::persistence(error),
    }
}

/// Preserve a uniqueness race as the semantic identifier conflict.
pub fn create_error(error: sqlx::Error, identifier: &Uri) -> SubscriptionError {
    if let sqlx::Error::Database(database_error) = &error
        && database_error.code().as_deref() == Some("23505")
    {
        SubscriptionError::already_exists(identifier)
    } else {
        persistence_error(error)
    }
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::failure::persistence_error;
    use andromeda_subscription::error::SubscriptionError;

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        assert!(matches!(persistence_error(sqlx::Error::PoolTimedOut), SubscriptionError::StoreSaturated));
        assert!(matches!(persistence_error(sqlx::Error::RowNotFound), SubscriptionError::Persistence(_)));
    }
}
