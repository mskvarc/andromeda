//! The admin console of the `PgBouncer` in front of a broker's PostgreSQL server: pausing, resuming, reconnecting and
//! killing one tenant database's pools, and listing the pooler's pools and clients.
//!
//! The console answers its commands over the simple query protocol only, so each call opens a connection of its own
//! and closes it again: the console is no pooled workload. Every control command names one database, so a test acts on
//! its own tenants' pools and leaves every other test's alone.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
    database_name::DatabaseName,
};
use andromeda_postgresql_seaquery::{
    command::Command,
    pooler_console::{PoolerCommand, PoolerDatabase, UnacceptedPoolerDatabase},
};
use sqlx::{
    Column,
    Connection,
    PgConnection,
    Row,
    TypeInfo,
    postgres::{PgRow, PgTypeInfo},
};
use std::num::ParseIntError;
use strum::{Display, IntoStaticStr};
use url::Url;

/// Why a console command failed.
#[derive(Debug, thiserror::Error)]
pub enum PoolerConsoleError {
    /// The stack names no pooler for the broker.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The console could not be reached, the command failed, or a column is missing or does not decode as the type
    /// it declares.
    #[error("the pooler admin console failed")]
    Console(#[from] sqlx::Error),
    /// A database name the console would read as more than one word.
    #[error(transparent)]
    UnacceptedDatabase(#[from] UnacceptedPoolerDatabase),
    /// A count column is declared with a type that holds no integer.
    #[error("the pooler console column {column} is declared {declared}, which holds no integer")]
    NotAnIntegerColumn {
        /// The column read.
        column: PoolCount,
        /// The type the console declared for it.
        declared: PgTypeInfo,
    },
    /// A count column sent as text does not hold an integer.
    #[error("the pooler console column {column} holds text that is not an integer")]
    NotAnInteger {
        /// The column read.
        column: PoolCount,
        /// Why the text does not parse.
        #[source]
        source: ParseIntError,
    },
}

/// A count column of `SHOW POOLS`, named as the console names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PoolCount {
    /// Client connections linked to a server connection.
    ClActive,
    /// Client connections waiting for a server connection.
    ClWaiting,
    /// Server connections running a client's transaction.
    SvActive,
    /// Server connections idle and ready for a client.
    SvIdle,
    /// Server connections released by a client and not yet reset.
    SvUsed,
}

/// One row of `SHOW POOLS`: the client and server connections of one (database, user) pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolRow {
    /// The pooled database.
    pub database: DatabaseName,
    /// Client connections linked to a server connection.
    pub clients_active: i64,
    /// Client connections waiting for a server connection.
    pub clients_waiting: i64,
    /// Server connections running a client's transaction.
    pub servers_active: i64,
    /// Server connections idle and ready for a client.
    pub servers_idle: i64,
    /// Server connections released by a client and not yet reset.
    pub servers_used: i64,
}

impl PoolRow {
    /// Every server connection the pool holds.
    #[must_use]
    pub const fn server_connections(&self) -> i64 {
        self.servers_active + self.servers_idle + self.servers_used
    }
}

/// A client of the admin console of one pooler.
#[derive(Debug, Clone)]
pub struct PoolerConsole {
    /// The console's URL (database `pgbouncer`).
    admin: Url,
}

impl PoolerConsole {
    /// The console of the pooler in front of the server of the broker of `role`.
    ///
    /// # Errors
    ///
    /// Returns [`PoolerConsoleError::Config`] when the stack names no such pooler.
    pub fn of(role: BrokerRole) -> Result<PoolerConsole, PoolerConsoleError> {
        let config = SuiteConfig::get().map_err(Clone::clone)?;
        Ok(PoolerConsole {
            admin: config.pooling()?.pooler(role)?.clone(),
        })
    }

    /// The URL a client of the pooler reaches `database` by, with the console's login.
    #[must_use]
    pub fn database_url(&self, database: &DatabaseName) -> Url {
        let mut url = self.admin.clone();
        url.set_path(&format!("/{database}"));
        url
    }

    /// `PAUSE <database>`.
    ///
    /// # Errors
    ///
    /// Returns [`PoolerConsoleError`] when the name is refused or the console fails.
    pub async fn pause(&self, database: &DatabaseName) -> Result<(), PoolerConsoleError> {
        self.control(PoolerCommand::Pause(PoolerDatabase::new(database.as_str())?)).await
    }

    /// `RESUME <database>`.
    ///
    /// # Errors
    ///
    /// Returns [`PoolerConsoleError`] when the name is refused or the console fails.
    pub async fn resume(&self, database: &DatabaseName) -> Result<(), PoolerConsoleError> {
        self.control(PoolerCommand::Resume(PoolerDatabase::new(database.as_str())?)).await
    }

    /// `RECONNECT <database>`.
    ///
    /// # Errors
    ///
    /// Returns [`PoolerConsoleError`] when the name is refused or the console fails.
    pub async fn reconnect(&self, database: &DatabaseName) -> Result<(), PoolerConsoleError> {
        self.control(PoolerCommand::Reconnect(PoolerDatabase::new(database.as_str())?)).await
    }

    /// `KILL <database>`.
    ///
    /// # Errors
    ///
    /// Returns [`PoolerConsoleError`] when the name is refused or the console fails.
    pub async fn kill(&self, database: &DatabaseName) -> Result<(), PoolerConsoleError> {
        self.control(PoolerCommand::Kill(PoolerDatabase::new(database.as_str())?)).await
    }

    /// Every pool `SHOW POOLS` lists.
    ///
    /// # Errors
    ///
    /// Returns [`PoolerConsoleError`] when the console fails or a row cannot be read.
    pub async fn pools(&self) -> Result<Vec<PoolRow>, PoolerConsoleError> {
        let mut connection = PgConnection::connect(self.admin.as_str()).await?;
        let rows = PoolerCommand::ShowPools.to_query().fetch_all(&mut connection).await;
        connection.close().await?;
        rows?.iter().map(pool_row).collect()
    }

    /// The pool `SHOW POOLS` lists for `database`, if the pooler holds one.
    ///
    /// # Errors
    ///
    /// Returns [`PoolerConsoleError`] when the console fails or a row cannot be read.
    pub async fn pool_of(&self, database: &DatabaseName) -> Result<Option<PoolRow>, PoolerConsoleError> {
        Ok(self.pools().await?.into_iter().find(|pool| pool.database == *database))
    }

    /// How many client connections `SHOW CLIENTS` lists for `database`, in any state.
    ///
    /// # Errors
    ///
    /// Returns [`PoolerConsoleError`] when the console fails or a row cannot be read.
    pub async fn clients_of(&self, database: &DatabaseName) -> Result<usize, PoolerConsoleError> {
        let mut connection = PgConnection::connect(self.admin.as_str()).await?;
        let rows = PoolerCommand::ShowClients.to_query().fetch_all(&mut connection).await;
        connection.close().await?;
        let databases = rows?.iter().map(|row| row.try_get::<String, _>("database")).collect::<Result<Vec<_>, _>>()?;
        Ok(databases.iter().filter(|name| name.as_str() == database.as_str()).count())
    }

    /// Runs one control command, which answers no rows.
    async fn control(&self, command: PoolerCommand) -> Result<(), PoolerConsoleError> {
        let mut connection = PgConnection::connect(self.admin.as_str()).await?;
        let outcome = command.to_query().execute(&mut connection).await;
        connection.close().await?;
        outcome?;
        Ok(())
    }
}

/// One `SHOW POOLS` row.
fn pool_row(row: &PgRow) -> Result<PoolRow, PoolerConsoleError> {
    Ok(PoolRow {
        database: DatabaseName::from(row.try_get::<String, _>("database")?),
        clients_active: integer_column(row, PoolCount::ClActive)?,
        clients_waiting: integer_column(row, PoolCount::ClWaiting)?,
        servers_active: integer_column(row, PoolCount::SvActive)?,
        servers_idle: integer_column(row, PoolCount::SvIdle)?,
        servers_used: integer_column(row, PoolCount::SvUsed)?,
    })
}

/// One count column, read as the type the console declares for it.
///
/// `PgBouncer` releases declare a count `int8`, `int4` or text, so the declared type picks the decoding; any other type,
/// or text holding no integer, fails the read rather than counting as zero.
fn integer_column(row: &PgRow, column: PoolCount) -> Result<i64, PoolerConsoleError> {
    let name: &'static str = column.into();
    let declared = row.try_column(name)?.type_info();
    match declared.name() {
        "INT8" => Ok(row.try_get::<i64, _>(name)?),
        "INT4" => Ok(i64::from(row.try_get::<i32, _>(name)?)),
        "INT2" => Ok(i64::from(row.try_get::<i16, _>(name)?)),
        "TEXT" | "VARCHAR" | "NAME" => console_integer(column, &row.try_get::<String, _>(name)?),
        _other => Err(PoolerConsoleError::NotAnIntegerColumn {
            column,
            declared: declared.clone(), // clone: the error outlives the row the type is borrowed from
        }),
    }
}

/// The integer a count column sent as text holds.
fn console_integer(column: PoolCount, text: &str) -> Result<i64, PoolerConsoleError> {
    text.trim().parse().map_err(|source| PoolerConsoleError::NotAnInteger { column, source })
}

#[cfg(test)]
mod tests {
    use crate::{
        database_name::DatabaseName,
        pooler_console::{PoolCount, PoolRow, PoolerConsole, PoolerConsoleError, console_integer},
    };
    use googletest::prelude::*;
    use url::Url;

    #[gtest]
    fn a_count_sent_as_text_reads_as_its_integer() {
        expect_that!(console_integer(PoolCount::SvIdle, " 12 ").ok(), some(eq(12)));
    }

    #[gtest]
    fn a_count_sent_as_text_that_holds_no_integer_fails_the_read() {
        expect_that!(
            console_integer(PoolCount::SvIdle, "many"),
            err(predicate(|error: &PoolerConsoleError| matches!(
                error,
                PoolerConsoleError::NotAnInteger { column: PoolCount::SvIdle, .. }
            )))
        );
    }

    #[gtest]
    fn a_count_column_is_named_as_the_console_names_it() {
        let names: Vec<&'static str> = [
            PoolCount::ClActive,
            PoolCount::ClWaiting,
            PoolCount::SvActive,
            PoolCount::SvIdle,
            PoolCount::SvUsed,
        ]
        .into_iter()
        .map(<&'static str>::from)
        .collect();
        expect_that!(
            names,
            elements_are![eq(&"cl_active"), eq(&"cl_waiting"), eq(&"sv_active"), eq(&"sv_idle"), eq(&"sv_used")]
        );
    }

    #[gtest]
    fn a_pool_holds_its_active_idle_and_used_servers() {
        let pool = PoolRow {
            database: DatabaseName::from("andromeda_tenant_a".to_owned()),
            clients_active: 1,
            clients_waiting: 4,
            servers_active: 2,
            servers_idle: 3,
            servers_used: 1,
        };
        expect_that!(pool.server_connections(), eq(6));
    }

    #[gtest]
    fn a_database_is_reached_through_the_pooler_with_the_console_login() -> Result<()> {
        let console = PoolerConsole {
            admin: Url::parse("postgres://andromeda:andromeda@pgbouncer:6432/pgbouncer")?,
        };
        let database = DatabaseName::from("andromeda_tenant_a".to_owned());
        verify_that!(
            console.database_url(&database).as_str(),
            eq("postgres://andromeda:andromeda@pgbouncer:6432/andromeda_tenant_a")
        )
    }
}
