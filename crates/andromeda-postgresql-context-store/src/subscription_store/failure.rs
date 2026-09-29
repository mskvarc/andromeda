//! How an adapter failure surfaces at the subscription policy boundary.

use crate::error::violates_unique_constraint;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_pool::is_pool_saturated;
use andromeda_subscription::error::SubscriptionError;
use std::{any::Any, fmt::Display};

/// The unique constraint migration 0023 names on `subscriptions.id`.
const UQ_SUBSCRIPTIONS_ID: &str = "uq_subscriptions_id";

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

    /// Migration 0023 names every constraint, so the duplicate-identifier
    /// refusal matches the declared name rather than one PostgreSQL generated.
    #[test]
    fn the_duplicate_id_refusal_names_the_constraint_migration_0023_declares() {
        let declaration = format!("CONSTRAINT {UQ_SUBSCRIPTIONS_ID} UNIQUE (id)");
        assert!(
            branch_migrations().iter().any(|migration| migration.sql.as_str().contains(&declaration)),
            "{declaration}"
        );
    }

    #[test]
    fn a_create_failure_that_is_no_unique_violation_stays_a_persistence_fault() {
        let identifier = "urn:ngsi-ld:Subscription:1".parse().expect("a valid identifier");
        assert!(matches!(create_error(sqlx::Error::RowNotFound, &identifier), SubscriptionError::Persistence(_)));
    }
}
