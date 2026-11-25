//! Running one storage-policy write without stalling the tenant behind it.
//!
//! The storage-policy reconciler writes through the tenant's serving pool,
//! while other replicas may be serving the same tenant: `ALTER TABLE ... SET`
//! takes an `AccessExclusiveLock` on the hypertable, and every statement that
//! reaches the hypertable after it queues behind that request for as long as
//! it waits. Each write therefore runs in a transaction of its own under
//! [`SCHEMA_CHANGE_LOCK_TIMEOUT`], set for that transaction only so no pooled
//! session keeps it, and is run again under [`SCHEMA_CHANGE_LOCK_RETRY`] when
//! the lock is refused.

use andromeda_postgresql_pool::schema_change_lock::{SCHEMA_CHANGE_LOCK_RETRY, SCHEMA_CHANGE_LOCK_TIMEOUT, is_lock_not_available};
use andromeda_postgresql_seaquery::{
    command::Command,
    setting::{Setting, SettingScope, set_config},
    statement::Statement,
};
use andromeda_retry::tokio_retry_delay::TokioRetryDelay;
use sea_query::{Query, SelectStatement};
use sqlx::{PgConnection, PgPool};
use std::ops::ControlFlow;

/// A storage-policy write: one statement, one command, or a read and the
/// writes it decides, all in the one transaction it is given.
pub(crate) trait BoundedWrite: Sync {
    /// What the write reports once its transaction commits.
    type Outcome: Send;

    /// Run the write on `connection`.
    fn execute(&self, connection: &mut PgConnection) -> impl Future<Output = Result<Self::Outcome, sqlx::Error>> + Send;
}

/// A write that is a statement with bound values.
pub(crate) struct StatementWrite<S>(pub(crate) S);

impl<S: Statement + Sync> BoundedWrite for StatementWrite<S> {
    type Outcome = ();

    async fn execute(&self, connection: &mut PgConnection) -> Result<(), sqlx::Error> {
        self.0.to_query().execute(connection).await.map(drop)
    }
}

/// A write that is a utility command.
pub(crate) struct CommandWrite<C>(pub(crate) C);

impl<C: Command + Sync> BoundedWrite for CommandWrite<C> {
    type Outcome = ();

    async fn execute(&self, connection: &mut PgConnection) -> Result<(), sqlx::Error> {
        self.0.to_query().execute(connection).await.map(drop)
    }
}

/// Run `write` in a transaction whose lock waits end after
/// [`SCHEMA_CHANGE_LOCK_TIMEOUT`], running it again while the lock is refused,
/// and report the outcome of the run that committed.
///
/// # Errors
///
/// Returns the write's error, or the refused lock once every attempt is spent.
pub(crate) async fn execute_with_bounded_lock_wait<W: BoundedWrite>(pool: &PgPool, write: &W) -> Result<W::Outcome, sqlx::Error> {
    let mut run = SCHEMA_CHANGE_LOCK_RETRY.start(&TokioRetryDelay);
    loop {
        let outcome = execute_under_lock_timeout(pool, write).await;
        if let ControlFlow::Break(settled) = run.settle(outcome, |error, _| is_lock_not_available(error)).await {
            return settled;
        }
    }
}

/// Run `write` once, in a transaction whose lock waits end after
/// [`SCHEMA_CHANGE_LOCK_TIMEOUT`].
async fn execute_under_lock_timeout<W: BoundedWrite>(pool: &PgPool, write: &W) -> Result<W::Outcome, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    lock_timeout_statement().to_query().execute(&mut *transaction).await?;
    let outcome = write.execute(&mut transaction).await?;
    transaction.commit().await?;
    Ok(outcome)
}

/// `SELECT set_config('lock_timeout', '<n>ms', true)`.
pub(crate) fn lock_timeout_statement() -> SelectStatement {
    Query::select()
        .expr(set_config(
            Setting::LockTimeout,
            &format!("{}ms", SCHEMA_CHANGE_LOCK_TIMEOUT.as_millis()),
            SettingScope::Transaction,
        ))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::bounded_lock_wait::lock_timeout_statement;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_lock_timeout_lasts_until_the_write_transaction_ends() {
        assert_eq!(
            lock_timeout_statement().to_string(PostgresQueryBuilder),
            "SELECT set_config('lock_timeout', '2000ms', TRUE)"
        );
    }
}
