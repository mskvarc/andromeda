//! Driving the `PgBouncer` admin console over the simple query protocol.
//!
//! `PgBouncer`'s admin pseudo-database answers `SHOW` and control statements
//! (`PAUSE`, `RESUME`, `RECONNECT`, `KILL`) only over the simple query
//! protocol, so every call here opens a fresh connection and runs the statement
//! through [`sqlx::raw_sql`], which never prepares. Connections are short-lived
//! by design: the admin console is not a pooled workload.

use sqlx::{Connection, PgConnection, Row, postgres::PgRow};

/// A client for one `PgBouncer` admin console.
pub struct PoolerConsole {
    /// The `postgres://.../pgbouncer` admin endpoint.
    admin_url: String,
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
    /// Returns the [`sqlx::Error`] from connecting or reading.
    pub async fn show_pools(&self) -> Result<Vec<PoolRow>, sqlx::Error> {
        let mut connection = self.connect().await?;
        let rows = sqlx::raw_sql("SHOW POOLS").fetch_all(&mut connection).await;
        let _ = connection.close().await;
        Ok(rows?.iter().map(pool_row).collect())
    }

    /// The total server connections `PgBouncer` holds against `PostgreSQL` across
    /// every pool except its own admin database.
    ///
    /// # Errors
    ///
    /// Returns the [`sqlx::Error`] from reading the pools.
    pub async fn total_server_connections(&self) -> Result<i64, sqlx::Error> {
        Ok(self
            .show_pools()
            .await?
            .iter()
            .filter(|pool| pool.database != "pgbouncer")
            .map(PoolRow::server_connections)
            .sum())
    }
}

/// Decode one `SHOW POOLS` row, defaulting any column `PgBouncer`'s build does not
/// send in the expected shape rather than failing the whole read.
fn pool_row(row: &PgRow) -> PoolRow {
    PoolRow {
        database: row.try_get::<String, _>("database").unwrap_or_default(),
        sv_active: integer_column(row, "sv_active"),
        sv_idle: integer_column(row, "sv_idle"),
        sv_used: integer_column(row, "sv_used"),
        cl_active: integer_column(row, "cl_active"),
        cl_waiting: integer_column(row, "cl_waiting"),
    }
}

/// Read one integer column, tolerating whether `PgBouncer` sent it as `int8`,
/// `int4`, or text.
fn integer_column(row: &PgRow, column: &str) -> i64 {
    if let Ok(value) = row.try_get::<i64, _>(column) {
        return value;
    }
    if let Ok(value) = row.try_get::<i32, _>(column) {
        return i64::from(value);
    }
    row.try_get::<String, _>(column)
        .ok()
        .and_then(|text| text.trim().parse::<i64>().ok())
        .unwrap_or(0)
}
