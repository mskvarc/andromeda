//! The session defaults the broker installs on every database it serves from.
//!
//! Two settings are part of the broker's own semantics rather than server
//! tuning, so the broker installs them itself, with `ALTER DATABASE ... SET`,
//! on the platform database and on every tenant database:
//!
//! - `synchronous_commit`, the durability of an acknowledged write
//!   ([`SynchronousCommit`]);
//! - `statement_timeout`, just past the HTTP request deadline
//!   ([`StatementTimeout`]): a statement whose request has already been
//!   answered with a timeout is work nobody waits for, and without a bound it
//!   keeps its connection, its locks and its share of the server until it ends.
//!
//! A database-level default is inherited by every session opened afterwards,
//! including one a transaction-pooling proxy opens, which a per-connection
//! `SET` would not survive. The defaults are reconciled each time a database is
//! opened, so a configuration change reaches databases created before it: the
//! stored defaults are read back and only a missing or different one is
//! written. Sessions opened before a write keep the values they started with,
//! which is why a caller that sees [`DefaultsReconciliation::Adjusted`] opens
//! its pool again.

use crate::{config::SynchronousCommit, error::ContextStoreError, schema_ddl::database_settings::set_database_default};
use andromeda_postgresql_seaquery::{advisory_lock::advisory_xact_lock, introspection::current_database, scalar_subquery::scalar_subquery};
use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Executor, PgPool, Row};
use std::time::Duration;

/// How far past the HTTP request deadline a statement may still run.
///
/// The request layer answers a request that outlives its deadline with a
/// timeout; the margin keeps the statement alive until that answer has been
/// given, so a request is never answered with a cancelled statement's error
/// instead, and covers the round trips around the statement.
pub const STATEMENT_TIMEOUT_MARGIN: Duration = Duration::from_secs(5);

/// The `statement_timeout` installed on each database: the HTTP request
/// deadline plus [`STATEMENT_TIMEOUT_MARGIN`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StatementTimeout(Duration);

impl StatementTimeout {
    /// The timeout for requests answered within `request_timeout`.
    #[must_use]
    pub const fn past_request_deadline(request_timeout: Duration) -> StatementTimeout {
        StatementTimeout(request_timeout.saturating_add(STATEMENT_TIMEOUT_MARGIN))
    }

    /// The timeout as a span.
    #[must_use]
    pub const fn duration(self) -> Duration {
        self.0
    }

    /// The value `ALTER DATABASE ... SET statement_timeout` takes, in whole
    /// milliseconds, the unit PostgreSQL stores it in.
    fn setting(self) -> String {
        format!("{}ms", self.0.as_millis())
    }
}

/// The session defaults one database carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DatabaseDefaults {
    /// Commit durability.
    pub synchronous_commit: SynchronousCommit,
    /// The bound on any one statement.
    pub statement_timeout: StatementTimeout,
}

/// One database-level default: a parameter and the value it is set to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseDefault {
    /// The run-time parameter.
    pub parameter: &'static str,
    /// Its value, as `ALTER DATABASE ... SET` takes and `pg_db_role_setting`
    /// stores it.
    pub value: String,
}

impl DatabaseDefault {
    /// The entry `pg_db_role_setting.setconfig` holds for this default.
    fn stored_form(&self) -> String {
        format!("{}={}", self.parameter, self.value)
    }
}

impl DatabaseDefaults {
    /// Every default, in the order they are written.
    #[must_use]
    pub fn entries(&self) -> [DatabaseDefault; 2] {
        [
            DatabaseDefault {
                parameter: "synchronous_commit",
                value: self.synchronous_commit.as_postgres_setting().to_owned(),
            },
            DatabaseDefault {
                parameter: "statement_timeout",
                value: self.statement_timeout.setting(),
            },
        ]
    }
}

/// Whether reconciling found the defaults already installed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefaultsReconciliation {
    /// Every default was already stored; nothing was written.
    Unchanged,
    /// At least one default was written; sessions opened before carry the old
    /// values.
    Adjusted,
}

/// Install `defaults` on the database `pool` connects to, writing only the
/// ones not already stored.
///
/// Several brokers open one database at once, and PostgreSQL does not let two
/// sessions write the same `pg_db_role_setting` row concurrently (the loser
/// fails with a unique violation or "tuple concurrently updated"), so the read
/// and the writes run in one transaction behind an advisory lock keyed on the
/// database.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the transaction, the lock, the
/// read or an `ALTER DATABASE` fails.
pub async fn reconcile_database_defaults(pool: &PgPool, defaults: &DatabaseDefaults) -> Result<DefaultsReconciliation, ContextStoreError> {
    let mut transaction = pool.begin().await?;
    let (sql, values) = stored_defaults_statement().build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(&mut *transaction).await?;
    let database: String = row.try_get("database")?;
    let (lock_sql, lock_values) = configuration_lock(&database).build_sqlx(PostgresQueryBuilder);
    sqlx::query_with(sqlx::AssertSqlSafe(lock_sql.as_str()), lock_values)
        .execute(&mut *transaction)
        .await?;
    // Read again under the lock: a broker that held it may have just written.
    let (sql, values) = stored_defaults_statement().build_sqlx(PostgresQueryBuilder);
    let stored: Option<Vec<String>> = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(&mut *transaction)
        .await?
        .try_get("settings")?;
    let entries = defaults.entries();
    let missing = missing_defaults(&entries, stored.as_deref().unwrap_or_default());
    if missing.is_empty() {
        transaction.rollback().await?;
        return Ok(DefaultsReconciliation::Unchanged);
    }
    for default in missing {
        transaction
            .execute(sqlx::AssertSqlSafe(set_database_default(&database, default.parameter, &default.value)))
            .await?;
    }
    transaction.commit().await?;
    Ok(DefaultsReconciliation::Adjusted)
}

/// The defaults of `desired` the stored `setconfig` entries do not hold.
fn missing_defaults<'d>(desired: &'d [DatabaseDefault], stored: &[String]) -> Vec<&'d DatabaseDefault> {
    desired.iter().filter(|default| !stored.contains(&default.stored_form())).collect()
}

/// The current database's name and its database-wide `setconfig` entries
/// (those of no particular role), `NULL` when it has none.
fn stored_defaults_statement() -> SelectStatement {
    let database_oid = Query::select()
        .column(Alias::new("oid"))
        .from(Alias::new("pg_database"))
        .and_where(Expr::col(Alias::new("datname")).eq(current_database()))
        .to_owned();
    let settings = Query::select()
        .column(Alias::new("setconfig"))
        .from(Alias::new("pg_db_role_setting"))
        .and_where(Expr::col(Alias::new("setrole")).eq(0))
        .and_where(Expr::col(Alias::new("setdatabase")).eq(scalar_subquery(database_oid)))
        .to_owned();
    Query::select()
        .expr_as(current_database(), Alias::new("database"))
        .expr_as(scalar_subquery(settings), Alias::new("settings"))
        .to_owned()
}

/// The transaction advisory lock that serializes configuring one database.
fn configuration_lock(database: &str) -> SelectStatement {
    Query::select()
        .expr(advisory_xact_lock(Expr::val(format!("andromeda:database-defaults:{database}"))))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::{
        config::SynchronousCommit,
        database_defaults::{DatabaseDefault, DatabaseDefaults, StatementTimeout, configuration_lock, missing_defaults, stored_defaults_statement},
    };
    use pretty_assertions::{assert_eq, assert_ne};
    use sea_query::PostgresQueryBuilder;
    use std::time::Duration;

    /// The defaults a broker with a 30 s request deadline installs.
    fn defaults() -> DatabaseDefaults {
        DatabaseDefaults {
            synchronous_commit: SynchronousCommit::On,
            statement_timeout: StatementTimeout::past_request_deadline(Duration::from_secs(30)),
        }
    }

    #[test]
    fn the_statement_timeout_is_the_request_deadline_plus_five_seconds() {
        assert_eq!(
            StatementTimeout::past_request_deadline(Duration::from_secs(30)).duration(),
            Duration::from_secs(35)
        );
        assert_eq!(
            StatementTimeout::past_request_deadline(Duration::from_millis(1500)).duration(),
            Duration::from_millis(6500)
        );
    }

    #[test]
    fn each_default_is_written_as_postgresql_stores_it() {
        let entries = defaults().entries();
        assert_eq!(
            entries.iter().map(DatabaseDefault::stored_form).collect::<Vec<_>>(),
            vec!["synchronous_commit=on", "statement_timeout=35000ms"]
        );
    }

    #[test]
    fn only_a_default_missing_or_stored_with_another_value_is_written() {
        let entries = defaults().entries();
        let stored = vec!["synchronous_commit=on".to_owned(), "statement_timeout=30000ms".to_owned()];
        let missing = missing_defaults(&entries, &stored);
        assert_eq!(missing.iter().map(|default| default.parameter).collect::<Vec<_>>(), vec!["statement_timeout"]);
        let stored = vec![
            "statement_timeout=35000ms".to_owned(),
            "search_path=public".to_owned(),
            "synchronous_commit=on".to_owned(),
        ];
        assert_eq!(missing_defaults(&entries, &stored), Vec::<&DatabaseDefault>::new());
        assert_eq!(missing_defaults(&entries, &[]).len(), 2);
    }

    #[test]
    fn the_stored_defaults_are_those_of_the_current_database_for_no_particular_role() {
        let sql = stored_defaults_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"current_database() AS "database""#), "{sql}");
        assert!(sql.contains(r#"FROM "pg_db_role_setting" WHERE "setrole" = 0"#), "{sql}");
        assert!(sql.contains(r#""datname" = current_database()"#), "{sql}");
        assert!(sql.contains(r#"AS "settings""#), "{sql}");
    }

    #[test]
    fn brokers_configuring_one_database_take_the_same_lock_and_other_databases_do_not_contend() {
        let (sql, values) = configuration_lock("andromeda").build(PostgresQueryBuilder);
        assert!(sql.contains("pg_advisory_xact_lock(hashtext("), "{sql}");
        assert_eq!(values, configuration_lock("andromeda").build(PostgresQueryBuilder).1);
        assert_ne!(values, configuration_lock("other").build(PostgresQueryBuilder).1);
    }
}
