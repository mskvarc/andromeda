//! Driving the `PgBouncer` admin console over the simple query protocol.
//!
//! `PgBouncer`'s admin pseudo-database answers `SHOW` and control statements
//! (`PAUSE`, `RESUME`, `RECONNECT`, `KILL`) only over the simple query
//! protocol, so every call here opens a fresh connection and runs the statement
//! through [`sqlx::raw_sql`], which never prepares. Connections are short-lived
//! by design: the admin console is not a pooled workload.

use sqlx::{Column, Connection, PgConnection, Row, TypeInfo, postgres::PgRow};
use std::num::ParseIntError;

/// A client for one `PgBouncer` admin console.
pub struct PoolerConsole {
    /// The `postgres://.../pgbouncer` admin endpoint.
    admin_url: String,
}

/// Why a console read failed.
#[derive(Debug, thiserror::Error)]
pub enum PoolerConsoleError {
    /// The console could not be reached, the statement failed, or a column is
    /// missing or does not decode as the type it declares.
    #[error("the pooler admin console read failed: {0}")]
    Console(#[from] sqlx::Error),
    /// A count column is declared with a type that holds no integer.
    #[error("the pooler console column {column} is declared {declared}, which holds no integer")]
    NotAnIntegerColumn {
        /// The column read.
        column: &'static str,
        /// The type the console declared for it.
        declared: String,
    },
    /// A count column sent as text does not hold an integer.
    #[error("the pooler console column {column} holds {text:?}, which is not an integer: {source}")]
    NotAnInteger {
        /// The column read.
        column: &'static str,
        /// The text the console sent.
        text: String,
        /// Why the text does not parse.
        #[source]
        source: ParseIntError,
    },
}

/// One row of `SHOW POOLS`: a per-(database, user) view of client and server
/// connection counts.
#[derive(Debug, Clone)]
pub struct PoolRow {
    /// The pooled database.
    pub database: String,
    /// Server connections actively executing a client query.
    pub sv_active: i64,
    /// Server connections idle and available for reuse.
    pub sv_idle: i64,
    /// Server connections leased to a client mid-transaction but not executing.
    pub sv_used: i64,
    /// Client connections actively linked to a server connection.
    pub cl_active: i64,
    /// Client connections waiting for a server connection.
    pub cl_waiting: i64,
}

/// One row of `SHOW CLIENTS`: a client connection logged in to `PgBouncer`.
#[derive(Debug, Clone)]
pub struct ClientRow {
    /// The database the client logged in to.
    pub database: String,
    /// The client's `application_name` startup parameter.
    pub application_name: String,
    /// The client's state (`active`, `waiting`, ...).
    pub state: String,
}

impl PoolRow {
    /// Every server connection this pool holds against `PostgreSQL`.
    #[must_use]
    pub const fn server_connections(&self) -> i64 {
        self.sv_active + self.sv_idle + self.sv_used
    }
}

impl PoolerConsole {
    /// A console over the given `postgres://.../pgbouncer` admin endpoint.
    pub fn new(admin_url: impl Into<String>) -> PoolerConsole {
        PoolerConsole { admin_url: admin_url.into() }
    }

    /// Open a fresh admin connection.
    async fn connect(&self) -> Result<PgConnection, sqlx::Error> {
        PgConnection::connect(&self.admin_url).await
    }

    /// Run one admin control statement that returns no rows.
    ///
    /// # Errors
    ///
    /// Returns the [`sqlx::Error`] from connecting or executing.
    pub async fn command(&self, statement: &str) -> Result<(), sqlx::Error> {
        // The admin statements are fixed control verbs and a database name from
        // the test's own tenant set, never user input; AssertSqlSafe is the
        // dynamic-string path `raw_sql` requires for a non-static query.
        let statement = sqlx::AssertSqlSafe(statement.to_owned());
        let mut connection = self.connect().await?;
        let outcome = sqlx::raw_sql(statement).execute(&mut connection).await.map(|_| ());
        let _ = connection.close().await;
        outcome
    }

    /// Pause every pool: new client transactions wait until [`resume`](Self::resume).
    ///
    /// # Errors
    ///
    /// Returns the [`sqlx::Error`] the console reported.
    pub async fn pause(&self) -> Result<(), sqlx::Error> {
        self.command("PAUSE").await
    }

    /// Resume every paused pool.
    ///
    /// # Errors
    ///
    /// Returns the [`sqlx::Error`] the console reported.
    pub async fn resume(&self) -> Result<(), sqlx::Error> {
        self.command("RESUME").await
    }

    /// Reconnect every server connection at the next opportunity.
    ///
    /// # Errors
    ///
    /// Returns the [`sqlx::Error`] the console reported.
    pub async fn reconnect(&self) -> Result<(), sqlx::Error> {
        self.command("RECONNECT").await
    }

    /// Immediately drop every client and server connection for one database.
    ///
    /// # Errors
    ///
    /// Returns the [`sqlx::Error`] the console reported.
    pub async fn kill(&self, database: &str) -> Result<(), sqlx::Error> {
        self.command(&format!("KILL {database}")).await
    }

    /// Resume one paused or killed database.
    ///
    /// # Errors
    ///
    /// Returns the [`sqlx::Error`] the console reported.
    pub async fn resume_database(&self, database: &str) -> Result<(), sqlx::Error> {
        self.command(&format!("RESUME {database}")).await
    }

    /// Read `SHOW POOLS`, one [`PoolRow`] per (database, user) pool.
    ///
    /// # Errors
    ///
    /// Returns [`PoolerConsoleError`] when connecting or reading fails, or when
    /// a column of a row is missing or unreadable.
    pub async fn show_pools(&self) -> Result<Vec<PoolRow>, PoolerConsoleError> {
        let mut connection = self.connect().await?;
        let rows = sqlx::raw_sql("SHOW POOLS").fetch_all(&mut connection).await;
        let _ = connection.close().await;
        rows?.iter().map(pool_row).collect()
    }

    /// Read `SHOW CLIENTS`, one [`ClientRow`] per client connection.
    ///
    /// # Errors
    ///
    /// Returns [`PoolerConsoleError`] when connecting or reading fails, or when
    /// a column of a row is missing or unreadable.
    pub async fn show_clients(&self) -> Result<Vec<ClientRow>, PoolerConsoleError> {
        let mut connection = self.connect().await?;
        let rows = sqlx::raw_sql("SHOW CLIENTS").fetch_all(&mut connection).await;
        let _ = connection.close().await;
        Ok(rows?.iter().map(client_row).collect::<Result<_, sqlx::Error>>()?)
    }

    /// The total server connections `PgBouncer` holds against `PostgreSQL` across
    /// every pool except its own admin database.
    ///
    /// # Errors
    ///
    /// Returns [`PoolerConsoleError`] from reading the pools.
    pub async fn total_server_connections(&self) -> Result<i64, PoolerConsoleError> {
        Ok(self
            .show_pools()
            .await?
            .iter()
            .filter(|pool| pool.database != "pgbouncer")
            .map(PoolRow::server_connections)
            .sum())
    }
}

/// Decode one `SHOW POOLS` row.
fn pool_row(row: &PgRow) -> Result<PoolRow, PoolerConsoleError> {
    Ok(PoolRow {
        database: row.try_get("database")?,
        sv_active: integer_column(row, "sv_active")?,
        sv_idle: integer_column(row, "sv_idle")?,
        sv_used: integer_column(row, "sv_used")?,
        cl_active: integer_column(row, "cl_active")?,
        cl_waiting: integer_column(row, "cl_waiting")?,
    })
}

/// Decode one `SHOW CLIENTS` row.
fn client_row(row: &PgRow) -> Result<ClientRow, sqlx::Error> {
    Ok(ClientRow {
        database: row.try_get("database")?,
        application_name: row.try_get("application_name")?,
        state: row.try_get("state")?,
    })
}

/// Read one count column as the type the console declares for it.
///
/// `PgBouncer` releases differ in declaring a count `int8`, `int4` or text, so
/// the declared type picks the decoding; a column of any other type, or text
/// that holds no integer, fails the read rather than counting as zero.
fn integer_column(row: &PgRow, column: &'static str) -> Result<i64, PoolerConsoleError> {
    match row.try_column(column)?.type_info().name() {
        "INT8" => Ok(row.try_get::<i64, _>(column)?),
        "INT4" => Ok(i64::from(row.try_get::<i32, _>(column)?)),
        "INT2" => Ok(i64::from(row.try_get::<i16, _>(column)?)),
        "TEXT" | "VARCHAR" | "NAME" => console_integer(column, row.try_get(column)?),
        declared => Err(PoolerConsoleError::NotAnIntegerColumn {
            column,
            declared: declared.to_owned(),
        }),
    }
}

/// The integer a count column sent as text holds.
fn console_integer(column: &'static str, text: String) -> Result<i64, PoolerConsoleError> {
    text.trim().parse().map_err(|source| PoolerConsoleError::NotAnInteger { column, text, source })
}

#[cfg(test)]
mod tests {
    use crate::pooler_probe::pooler_console::{PoolerConsoleError, console_integer};

    #[test]
    fn a_count_sent_as_text_reads_as_its_integer() {
        assert!(matches!(console_integer("sv_idle", " 12 ".to_owned()), Ok(12)));
    }

    #[test]
    fn a_count_sent_as_text_that_holds_no_integer_fails_the_read() {
        assert!(matches!(
            console_integer("sv_idle", "many".to_owned()),
            Err(PoolerConsoleError::NotAnInteger { column: "sv_idle", .. })
        ));
    }
}
