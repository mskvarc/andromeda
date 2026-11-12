//! How a client write of a subscription ends its transaction.
//!
//! An entity subscription commits as it stands. A context-source subscription
//! is ordered against every registration write by a change clock revision
//! (clauses 5.11.2.4 and 5.11.3.4), drawn by the stamp its commit carries, so
//! the clock's row is held only while the server runs the stamp and the commit.

use crate::{store_fault::persistence_error, subscription_store::csource_statements::csource_revision_stamp};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_pool::pool_share::SharedTransaction;
use andromeda_postgresql_seaquery::committing_statement::CommittingStatement;
use andromeda_subscription::error::SubscriptionError;
use sqlx::{Postgres, Transaction};

/// Commit the write of an entity subscription, which draws no revision.
///
/// # Errors
///
/// Returns the persistence failure of a commit that fails.
pub(super) async fn commit_entity_write(
    transaction: Transaction<'static, Postgres>,
    _target: &ContextTarget,
    _subscription_key: i64,
) -> Result<(), SubscriptionError> {
    transaction.commit().await.map_err(|error| persistence_error::<SubscriptionError>(error))
}

/// Stamp the write of the context-source subscription `subscription_key`
/// names with the revision its commit draws, in the round trip that commits
/// it.
///
/// # Errors
///
/// Returns the persistence failure of a stamp or a commit that fails.
pub(super) async fn commit_csource_write(transaction: SharedTransaction, target: &ContextTarget, subscription_key: i64) -> Result<(), SubscriptionError> {
    let stamp = CommittingStatement::new(&csource_revision_stamp(target, subscription_key)).map_err(|error| persistence_error::<SubscriptionError>(error))?;
    transaction
        .commit_executing(&stamp)
        .await
        .map_err(|error| persistence_error::<SubscriptionError>(error))
}
