//! Holding a tenant database's context source change clock with one session, connected straight to the broker's
//! server with its administrative login, that locks the clock's one row in a transaction and sleeps, so every
//! registration write and every context source subscription write of that tenant waits on the row until the sleep ends.
//!
//! The session reaches the server directly, not through the pooler, so it takes none of the server connections the
//! pooler keeps for the tenant's database.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
    database_name::DatabaseName,
};
use andromeda_postgresql_seaquery::{catalog_function::pg_sleep, statement::Statement};
use sea_query::{Alias, Expr, LockType, Query, SelectStatement};
use sqlx::{Connection, PgConnection};
use std::time::Duration;
use tokio::{
    sync::oneshot,
    task::{JoinError, JoinHandle},
};
use url::Url;

/// The relation every write that orders registrations against context source subscriptions advances.
const CHANGE_CLOCK: &str = "context_source_change_clock";

/// The clock's one column, the last revision drawn.
const REVISION: &str = "revision";

/// Why the clock could not be held.
#[derive(Debug, thiserror::Error)]
pub enum ChangeClockHoldError {
    /// The stack names no server for the broker.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The tenant database could not be reached, or a statement of the hold failed.
    #[error("the tenant database could not be held")]
    Server(#[from] sqlx::Error),
    /// The holding session ended abnormally.
    #[error("the holding session ended abnormally")]
    Session(#[from] JoinError),
    /// The holding session ended before it took the row lock, with no error to tell why.
    #[error("the holding session ended before it locked the change clock")]
    NotLocked,
}

/// A session holding one tenant database's change clock until its sleep ends.
#[derive(Debug)]
pub struct ChangeClockHold {
    /// The holding session, answering how its transaction ended.
    session: JoinHandle<Result<(), sqlx::Error>>,
}

impl ChangeClockHold {
    /// Locks the change clock of `database` on the server of the broker of `role` for `held_for`, and returns once
    /// the row lock is taken.
    ///
    /// # Errors
    ///
    /// Returns [`ChangeClockHoldError`] when the stack names no such server, or the session cannot connect or lock
    /// the row.
    pub async fn seize(role: BrokerRole, database: &DatabaseName, held_for: Duration) -> Result<ChangeClockHold, ChangeClockHoldError> {
        let url = tenant_database_url(role, database)?;
        let (locked, locked_signal) = oneshot::channel();
        let session = tokio::spawn(async move {
            let mut connection = PgConnection::connect(url.as_str()).await?;
            let mut transaction = connection.begin().await?;
            lock_statement().to_query().execute(&mut *transaction).await?;
            if locked.send(()).is_err() {
                return transaction.rollback().await;
            }
            sleep_statement(held_for).to_query().execute(&mut *transaction).await?;
            transaction.commit().await?;
            connection.close().await
        });
        match locked_signal.await {
            Ok(()) => Ok(ChangeClockHold { session }),
            Err(_never_locked) => match session.await? {
                Ok(()) => Err(ChangeClockHoldError::NotLocked),
                Err(error) => Err(error.into()),
            },
        }
    }

    /// Waits for the sleep to end and the lock to be released.
    ///
    /// # Errors
    ///
    /// Returns [`ChangeClockHoldError`] when the session ended abnormally or a statement of the hold failed.
    pub async fn release(self) -> Result<(), ChangeClockHoldError> {
        Ok(self.session.await??)
    }
}

/// The URL of `database` on the server of the broker of `role`, with the server's administrative login.
fn tenant_database_url(role: BrokerRole, database: &DatabaseName) -> Result<Url, ConfigError> {
    let config = SuiteConfig::get().map_err(Clone::clone)?;
    let mut url = config.pooling()?.server(role)?.clone();
    url.set_path(&format!("/{database}"));
    Ok(url)
}

/// The lock of the clock's one row that every advance of the clock waits on.
fn lock_statement() -> SelectStatement {
    Query::select()
        .column(Alias::new(REVISION))
        .from(Alias::new(CHANGE_CLOCK))
        .lock(LockType::Update)
        .to_owned()
}

/// The sleep the lock is held through.
fn sleep_statement(held_for: Duration) -> SelectStatement {
    Query::select().expr(pg_sleep(Expr::val(held_for.as_secs_f64()))).to_owned()
}

#[cfg(test)]
mod tests {
    use crate::change_clock_hold::{lock_statement, sleep_statement};
    use googletest::prelude::*;
    use sea_query::PostgresQueryBuilder;
    use std::time::Duration;

    #[gtest]
    fn the_hold_locks_the_one_row_of_the_change_clock_for_update() -> Result<()> {
        verify_that!(
            lock_statement().to_string(PostgresQueryBuilder),
            eq(r#"SELECT "revision" FROM "context_source_change_clock" FOR UPDATE"#)
        )
    }

    #[gtest]
    fn the_hold_sleeps_for_its_whole_duration_in_seconds() -> Result<()> {
        verify_that!(
            sleep_statement(Duration::from_millis(8_500)).to_string(PostgresQueryBuilder),
            eq("SELECT pg_sleep(8.5)")
        )
    }
}
