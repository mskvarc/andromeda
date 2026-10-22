//! How an adapter failure surfaces at the subscription policy boundary.

use crate::store_fault::persistence_error;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_pool::sql_state::violates_unique_constraint;
use andromeda_subscription::error::SubscriptionError;

/// The unique constraint the migrations name on `subscriptions.id`.
const UQ_SUBSCRIPTIONS_ID: &str = "uq_subscriptions_id";

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
        persistence_error::<SubscriptionError>(error)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        migration_set::branch::branch_migrations,
        store_fault::persistence_error,
        subscription_store::failure::{UQ_SUBSCRIPTIONS_ID, create_error},
    };
    use andromeda_subscription::error::SubscriptionError;

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        assert!(matches!(
            persistence_error::<SubscriptionError>(sqlx::Error::PoolTimedOut),
            SubscriptionError::StoreSaturated
        ));
        assert!(matches!(
            persistence_error::<SubscriptionError>(sqlx::Error::RowNotFound),
            SubscriptionError::Persistence(_)
        ));
    }

    /// The migrations name every constraint, so the duplicate-identifier
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
