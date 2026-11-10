//! The client backends of the PostgreSQL server a broker owns, read from `pg_stat_activity` over a direct connection
//! with the server's administrative login, and closed with `pg_terminate_backend`.
//!
//! Each broker of the pgbouncer stack owns its server, so every client backend on it serves that broker: its request
//! pools through the pooler, its platform and maintenance connections, and nothing of another test's broker. The
//! session reading the count is not counted.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
    database_name::DatabaseName,
};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    catalog_function::{backend_pid, terminate_backend},
    pg_array::{any, text_array},
    statement::Statement,
    system_catalog::PgStatActivity,
};
use sea_query::{Expr, ExprTrait, Query, SelectStatement};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

/// The longest a count waits for its one connection.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

/// The `backend_type` of a session a client opened.
const CLIENT_BACKEND: &str = "client backend";

/// The `wait_event_type` of a session waiting on a heavyweight lock, a row lock among them.
const LOCK_WAIT: &str = "Lock";

/// Which client backends a count covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendScope {
    /// Every client backend of the server.
    WholeServer,
    /// The client backends connected to a tenant database: every database but the platform database the broker
    /// keeps its registry in.
    TenantDatabases,
    /// The client backends connected to one database.
    Database(DatabaseName),
    /// The client backends connected to one database that wait on a heavyweight lock.
    LockWaiters(DatabaseName),
}

/// Why the server's backends could not be read or closed.
#[derive(Debug, thiserror::Error)]
pub enum ServerBackendsError {
    /// The stack names no server for the broker.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The server could not be reached or the statement failed.
    #[error("the PostgreSQL server could not be read")]
    Server(#[from] sqlx::Error),
    /// A count the server answered is below zero.
    #[error("the server counted {0} backends")]
    NegativeCount(i64),
}

/// A direct connection to the PostgreSQL server of one broker.
#[derive(Debug, Clone)]
pub struct ServerBackends {
    /// One connection, opened on first use.
    pool: PgPool,
    /// The platform database: the one the server URL names.
    platform: DatabaseName,
}

impl ServerBackends {
    /// The server of the broker of `role`.
    ///
    /// # Errors
    ///
    /// Returns [`ServerBackendsError::Config`] when the stack names no such server.
    pub fn of(role: BrokerRole) -> Result<ServerBackends, ServerBackendsError> {
        let config = SuiteConfig::get().map_err(Clone::clone)?;
        let server = config.servers().server(role)?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(ACQUIRE_TIMEOUT)
            .connect_lazy(server.as_str())?;
        let platform = DatabaseName::from(server.path().trim_start_matches('/').to_owned());
        Ok(ServerBackends { pool, platform })
    }

    /// How many client backends `scope` covers now.
    ///
    /// # Errors
    ///
    /// Returns [`ServerBackendsError`] when the server cannot be read.
    pub async fn count(&self, scope: &BackendScope) -> Result<u64, ServerBackendsError> {
        let count: i64 = count_statement(scope, &self.platform).to_query_scalar().fetch_one(&self.pool).await?;
        u64::try_from(count).map_err(|_| ServerBackendsError::NegativeCount(count))
    }

    /// Closes every client backend connected to any of `databases`, answering how many were closed.
    ///
    /// # Errors
    ///
    /// Returns [`ServerBackendsError`] when the server cannot be reached.
    pub async fn terminate(&self, databases: &[DatabaseName]) -> Result<usize, ServerBackendsError> {
        let names = databases.iter().map(DatabaseName::as_str).collect::<Vec<_>>();
        let statement = Query::select()
            .expr(terminate_backend(Expr::col(PgStatActivity::Pid)))
            .from(PgStatActivity::Table)
            .and_where(Expr::col(PgStatActivity::BackendType).eq(CLIENT_BACKEND))
            .and_where(Expr::col(PgStatActivity::Datname).eq(any(text_array(&names))))
            .to_owned();
        let closed: Vec<bool> = statement.to_query_scalar().fetch_all(&self.pool).await?;
        Ok(closed.into_iter().filter(|terminated| *terminated).count())
    }
}

/// The count of the client backends `scope` covers, the reading session left out.
fn count_statement(scope: &BackendScope, platform: &DatabaseName) -> SelectStatement {
    let mut statement = Query::select()
        .expr(count_all())
        .from(PgStatActivity::Table)
        .and_where(Expr::col(PgStatActivity::BackendType).eq(CLIENT_BACKEND))
        .and_where(Expr::col(PgStatActivity::Pid).ne(backend_pid()))
        .to_owned();
    match scope {
        BackendScope::WholeServer => statement,
        BackendScope::TenantDatabases => statement.and_where(Expr::col(PgStatActivity::Datname).ne(platform.as_str())).to_owned(),
        BackendScope::Database(database) => statement.and_where(Expr::col(PgStatActivity::Datname).eq(database.as_str())).to_owned(),
        BackendScope::LockWaiters(database) => statement
            .and_where(Expr::col(PgStatActivity::Datname).eq(database.as_str()))
            .and_where(Expr::col(PgStatActivity::WaitEventType).eq(LOCK_WAIT))
            .to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        database_name::DatabaseName,
        server_backends::{BackendScope, count_statement},
    };
    use googletest::prelude::*;
    use sea_query::PostgresQueryBuilder;

    #[gtest]
    fn the_lock_waiters_of_a_database_are_its_client_backends_waiting_on_a_lock() -> Result<()> {
        let platform = DatabaseName::from("andromeda".to_owned());
        let tenant = DatabaseName::from("andromeda_tenant_acme_0badc0de".to_owned());
        let sql = count_statement(&BackendScope::LockWaiters(tenant), &platform).to_string(PostgresQueryBuilder);
        verify_that!(
            sql,
            eq(concat!(
                r#"SELECT COUNT(*) FROM "pg_stat_activity" WHERE "backend_type" = 'client backend' AND "pid" <> pg_backend_pid() "#,
                r#"AND "datname" = 'andromeda_tenant_acme_0badc0de' AND "wait_event_type" = 'Lock'"#
            ))
        )
    }
}
