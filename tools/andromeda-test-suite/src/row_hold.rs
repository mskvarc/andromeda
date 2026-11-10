//! Holding rows of a tenant database with one session, connected straight to the broker's server with its
//! administrative login, that locks them in a transaction and sleeps, so every write of those rows and every
//! conflicting lock the broker takes on them waits until the sleep ends.
//!
//! The session reaches the server directly, not through a pooler, so it takes none of the server connections a pooler
//! keeps for the tenant's database.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
    database_name::DatabaseName,
};
use andromeda_postgresql_seaquery::{catalog_function::pg_sleep, statement::Statement};
use sea_query::{Expr, Query, SelectStatement};
use sqlx::{Connection, PgConnection};
use std::time::Duration;
use tokio::{
    sync::oneshot,
    task::{JoinError, JoinHandle},
};

/// Why the rows could not be held.
#[derive(Debug, thiserror::Error)]
pub enum RowHoldError {
    /// The stack names no server for the broker.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The tenant database could not be reached, or a statement of the hold failed.
    #[error("the tenant database could not be held")]
    Server(#[from] sqlx::Error),
    /// The holding session ended abnormally.
    #[error("the holding session ended abnormally")]
    Session(#[from] JoinError),
    /// The holding session ended before it took the row locks, with no error to tell why.
    #[error("the holding session ended before it locked its rows")]
    NotLocked,
    /// The lock matched no row, so nothing would have been held.
    #[error("the lock matched no row of the tenant database")]
    NoRow,
}

/// A session holding rows of one tenant database until its sleep ends.
#[derive(Debug)]
pub struct RowHold {
    /// The holding session, answering how its transaction ended.
    session: JoinHandle<Result<(), sqlx::Error>>,
}

impl RowHold {
    /// Runs `lock` in `database` on the server of the broker of `role` and holds the rows it locks for `held_for`,
    /// returning once they are locked.
    ///
    /// # Errors
    ///
    /// Returns [`RowHoldError`] when the stack names no such server, the session cannot connect or run `lock`, or
    /// `lock` matches no row.
    pub async fn seize(role: BrokerRole, database: &DatabaseName, lock: SelectStatement, held_for: Duration) -> Result<RowHold, RowHoldError> {
        let url = SuiteConfig::get().map_err(Clone::clone)?.servers().database(role, database)?;
        let (locked, locked_signal) = oneshot::channel();
        let session = tokio::spawn(async move {
            let mut connection = PgConnection::connect(url.as_str()).await?;
            let mut transaction = connection.begin().await?;
            let rows = lock.to_query().fetch_all(&mut *transaction).await?.len();
            if locked.send(rows).is_err() || rows == 0 {
                return transaction.rollback().await;
            }
            sleep_statement(held_for).to_query().execute(&mut *transaction).await?;
            transaction.commit().await?;
            connection.close().await
        });
        match locked_signal.await {
            Ok(0) => {
                session.await??;
                Err(RowHoldError::NoRow)
            }
            Ok(_) => Ok(RowHold { session }),
            Err(_never_locked) => match session.await? {
                Ok(()) => Err(RowHoldError::NotLocked),
                Err(error) => Err(error.into()),
            },
        }
    }

    /// Waits for the sleep to end and the locks to be released.
    ///
    /// # Errors
    ///
    /// Returns [`RowHoldError`] when the session ended abnormally or a statement of the hold failed.
    pub async fn release(self) -> Result<(), RowHoldError> {
        Ok(self.session.await??)
    }
}

/// The sleep the locks are held through.
fn sleep_statement(held_for: Duration) -> SelectStatement {
    Query::select().expr(pg_sleep(Expr::val(held_for.as_secs_f64()))).to_owned()
}

#[cfg(test)]
mod tests {
    use crate::row_hold::sleep_statement;
    use googletest::prelude::*;
    use sea_query::PostgresQueryBuilder;
    use std::time::Duration;

    #[gtest]
    fn the_hold_sleeps_for_its_whole_duration_in_seconds() -> Result<()> {
        verify_that!(
            sleep_statement(Duration::from_millis(8_500)).to_string(PostgresQueryBuilder),
            eq("SELECT pg_sleep(8.5)")
        )
    }
}
