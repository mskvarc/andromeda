//! Creating and dropping the database behind one tenant.
//!
//! Every database the broker creates or drops goes through here. Naming a
//! tenant's database is [`crate::tenant_database_naming`]'s concern, migrating
//! it is [`crate::tenant_migration`]'s, and its session defaults are
//! [`crate::database_defaults`]'s.

use crate::error::ContextStoreError;
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_pool::{database_url::DatabaseUrl, sql_state::SqlState, transaction_rollback::is_transaction_rolled_back};
use andromeda_postgresql_seaquery::{
    command::Command,
    database_definition::{ConnectedSessions, DatabaseDefinition},
    literal::int_literal,
    statement::Statement,
    system_catalog::PgDatabase,
};
use andromeda_retry::{retry_policy::RetryPolicy, schedule::BackoffSchedule, tokio_retry_delay::TokioRetryDelay};
use sea_query::{Alias, Expr, ExprTrait, Query};
use sqlx::{Connection, PgConnection};
use std::{num::NonZeroU32, time::Duration};

/// Create the named database if it does not already exist, from a connection
/// to the database `base_url` names. `CREATE DATABASE` runs from any database,
/// and a tenant's database is never the one `base_url` names, so the server
/// needs no database set aside for administration.
///
/// No locale or collation is specified, so the database inherits the cluster's.
/// That is deliberate: every column whose comparison decides NGSI-LD behaviour
/// declares `COLLATE "C"` itself, so the database default governs nothing the
/// specification defines, and pinning it here would hide an omitted column
/// declaration on databases Andromeda creates while leaving it live on every
/// database an operator, a managed service or a restore provisions.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the administrative connection
/// cannot be opened, when the existence probe or the `CREATE DATABASE` fails,
/// or when the connection cannot be closed cleanly.
pub async fn create_database_if_missing(base_url: &DatabaseUrl, database_name: &TenantDatabaseName) -> Result<(), ContextStoreError> {
    let database_name = database_name.as_str();
    let mut conn = PgConnection::connect(base_url.as_ref().as_str()).await?;
    let exists = database_exists_statement(database_name).to_query().fetch_optional(&mut conn).await?.is_some();
    if !exists {
        create_database_with_retry(&mut conn, database_name).await?;
    }
    conn.close().await?;
    Ok(())
}

/// The outcome of dropping a tenant's database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseDrop {
    /// The database is gone, whether dropped now or missing already.
    Dropped,
    /// A session is connected to the database, so it was left in place.
    InUse,
}

/// Drop the named database unless a session is connected to it, from a
/// connection to the database `base_url` names, which is never a tenant's.
///
/// The drop never forces sessions out, so a database any broker still holds a
/// connection to is left in place (SQLSTATE 55006, `object_in_use`).
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the administrative connection
/// cannot be opened or closed, or when the drop fails for any reason other
/// than a connected session.
pub async fn drop_database_unless_in_use(base_url: &DatabaseUrl, database_name: &TenantDatabaseName) -> Result<DatabaseDrop, ContextStoreError> {
    let mut conn = PgConnection::connect(base_url.as_ref().as_str()).await?;
    let outcome = match DatabaseDefinition::drop(Alias::new(database_name.as_str()), ConnectedSessions::Refuse)
        .to_query()
        .execute(&mut conn)
        .await
    {
        Ok(_) => Ok(DatabaseDrop::Dropped),
        Err(error) if SqlState::of(&error) == Some(SqlState::ObjectInUse) => Ok(DatabaseDrop::InUse),
        Err(error) => Err(error.into()),
    };
    conn.close().await?;
    outcome
}

/// The retry a contended `CREATE DATABASE` runs under: six attempts a quarter
/// of a second apart.
const CREATE_DATABASE_RETRY: RetryPolicy = RetryPolicy {
    attempts: NonZeroU32::MIN.saturating_add(5),
    schedule: BackoffSchedule::constant(Duration::from_millis(250)),
};

/// Execute `CREATE DATABASE`, retrying the transient contention that arises when
/// many tenants are provisioned at once.
///
/// PostgreSQL copies a template database for every `CREATE DATABASE` and rejects
/// one whose template is being read by a concurrent create with SQLSTATE 55006
/// (`object_in_use`, "source database ... is being accessed by other users"); a
/// deadlock (40P01), a serialization failure (40001) or momentary connection
/// exhaustion (53300) is likewise transient. A broker that materialises many
/// tenant databases at once meets this contention in bursts,
/// which otherwise surfaces to the client as a 500 on the first request in the
/// new tenant. A create that lost the race to a concurrent create of the same
/// name (42P04) finds its database already made, which is success.
async fn create_database_with_retry(conn: &mut PgConnection, database_name: &str) -> Result<(), ContextStoreError> {
    CREATE_DATABASE_RETRY
        .run(
            &TokioRetryDelay,
            |error, _| is_transient_create_failure(error),
            async |_| match DatabaseDefinition::create(Alias::new(database_name)).to_query().execute(&mut *conn).await {
                Ok(_) => Ok(()),
                Err(error) if is_database_already_exists(&error) => Ok(()),
                Err(error) => Err(error),
            },
        )
        .await?;
    Ok(())
}

/// Whether a `CREATE DATABASE` failure is transient contention a retry can
/// clear: a template in use by a concurrent create, a serialization failure, a
/// deadlock or too many connections.
fn is_transient_create_failure(error: &sqlx::Error) -> bool {
    is_transaction_rolled_back(error) || matches!(SqlState::of(error), Some(SqlState::ObjectInUse | SqlState::TooManyConnections))
}

/// Whether a `CREATE DATABASE` failed because a concurrent provisioner already
/// created the database.
fn is_database_already_exists(error: &sqlx::Error) -> bool {
    SqlState::of(error) == Some(SqlState::DuplicateDatabase)
}

/// The `SELECT 1 FROM pg_database WHERE datname = $1` existence probe.
///
/// `pg_database` is a PostgreSQL system catalog, not part of the application
/// schema, so it is named by aliases; the database name is a bound value.
fn database_exists_statement(database_name: &str) -> sea_query::SelectStatement {
    Query::select()
        .expr(int_literal(1))
        .from(PgDatabase::Table)
        .and_where(Expr::col(PgDatabase::Datname).eq(database_name))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::tenant_database::{CREATE_DATABASE_RETRY, database_exists_statement, is_database_already_exists, is_transient_create_failure};
    use andromeda_postgresql_pool::fake_database_error::FakeDatabaseError;
    use andromeda_retry::schedule::BackoffSchedule;
    use sea_query::PostgresQueryBuilder;
    use std::time::Duration;

    /// A database error reporting the given SQLSTATE.
    fn database_error(code: &str) -> sqlx::Error {
        FakeDatabaseError::new("refused", code).into_sqlx()
    }

    #[test]
    fn the_existence_probe_reads_the_catalog_by_bound_name() {
        let (sql, values) = database_exists_statement("andromeda_acme").build(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "pg_database""#), "{sql}");
        assert!(sql.contains(r#""datname" = $1"#), "{sql}");
        assert_eq!(values.0.len(), 1, "the name is bound, not interpolated: {sql}");
    }

    #[test]
    fn only_the_concurrent_create_database_sqlstates_are_retried() {
        for code in ["55006", "40001", "40P01", "53300"] {
            assert!(is_transient_create_failure(&database_error(code)), "{code}");
        }
        for code in ["42P04", "42501", "23505", "3D000", "08006"] {
            assert!(!is_transient_create_failure(&database_error(code)), "{code}");
        }
        assert!(!is_transient_create_failure(&sqlx::Error::PoolTimedOut));
    }

    #[test]
    fn only_duplicate_database_is_treated_as_already_created() {
        assert!(is_database_already_exists(&database_error("42P04")));
        assert!(!is_database_already_exists(&database_error("55006")));
        assert!(!is_database_already_exists(&sqlx::Error::PoolTimedOut));
    }

    #[test]
    fn a_create_runs_six_attempts_a_quarter_second_apart() {
        pretty_assertions::assert_eq!(CREATE_DATABASE_RETRY.attempts.get(), 6);
        pretty_assertions::assert_eq!(CREATE_DATABASE_RETRY.schedule, BackoffSchedule::constant(Duration::from_millis(250)));
    }
}
