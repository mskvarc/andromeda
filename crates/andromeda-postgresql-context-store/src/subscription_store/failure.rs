//! How an adapter failure surfaces at the subscription policy boundary.

use crate::{
    error::violates_unique_constraint,
    store_fault::{StoreFault, persistence_failure},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_subscription::error::SubscriptionError;

/// The unique constraint migration 0024 names on `subscriptions.id`.
const UQ_SUBSCRIPTIONS_ID: &str = "uq_subscriptions_id";

/// Flatten an adapter failure at the policy boundary.
///
/// Classify an adapter failure at the port boundary: backpressure keeps the
/// port's retryable category and every fault becomes its persistence failure.
#[track_caller]
pub fn persistence_error(error: impl StoreFault) -> SubscriptionError {
    SubscriptionError::from(persistence_failure(&error))
}

/// Preserve a uniqueness race on the identifier as the semantic identifier
/// conflict.
///
/// Only the identifier's own constraint is the client's conflict: a violation
/// of the surrogate key is the store minting a key it already holds, which no
/// retry of the client's request under another identifier would avoid, so it
/// stays a persistence fault rather than a 409 naming an identifier nobody
/// used.
pub fn create_error(error: sqlx::Error, identifier: &Identifier) -> SubscriptionError {
    if violates_unique_constraint(&error, UQ_SUBSCRIPTIONS_ID) {
        SubscriptionError::already_exists(identifier)
    } else {
        persistence_error(error)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        migration_set::branch::branch_migrations,
        subscription_store::failure::{UQ_SUBSCRIPTIONS_ID, create_error, persistence_error},
    };
    use andromeda_subscription::error::SubscriptionError;

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        assert!(matches!(persistence_error(sqlx::Error::PoolTimedOut), SubscriptionError::StoreSaturated));
        assert!(matches!(persistence_error(sqlx::Error::RowNotFound), SubscriptionError::Persistence(_)));
    }

    /// Migration 0024 names every constraint, so the duplicate-identifier
    /// refusal matches the declared name rather than one PostgreSQL generated.
    #[test]
    fn the_duplicate_id_refusal_names_the_constraint_migration_0024_declares() {
        let declaration = format!("CONSTRAINT {UQ_SUBSCRIPTIONS_ID} UNIQUE (id)").to_ascii_uppercase();
        assert!(
            branch_migrations()
                .iter()
                .any(|migration| migration.sql.as_str().to_ascii_uppercase().contains(&declaration)),
            "{declaration}"
        );
    }

    #[test]
    fn a_create_failure_that_is_no_unique_violation_stays_a_persistence_fault() {
        let identifier = "urn:ngsi-ld:Subscription:1".parse().expect("a valid identifier");
        assert!(matches!(create_error(sqlx::Error::RowNotFound, &identifier), SubscriptionError::Persistence(_)));
    }
}
