//! How an adapter failure surfaces at the subscription policy boundary.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_subscription::error::SubscriptionError;
use std::fmt::Display;

/// Flatten an adapter failure at the policy boundary.
pub fn persistence_error(error: impl Display) -> SubscriptionError {
    SubscriptionError::persistence(error.to_string())
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
