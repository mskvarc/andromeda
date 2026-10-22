//! Finding and force-dropping the databases a test run owns.

use andromeda_postgresql_seaquery::{
    command::Command,
    database_definition::{ConnectedSessions, DatabaseDefinition},
    introspection::current_database,
    statement::Statement,
    system_catalog::PgDatabase,
};
use sea_query::{Alias, Expr, ExprTrait, Query};
use sqlx::PgPool;

/// Whether the database the pool is connected to is among those listed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlDatabase {
    /// The pool's own database is listed when it matches.
    Listed,
    /// The pool's own database is never listed.
    Excluded,
}

/// The names of the databases matching the `LIKE` pattern `pattern`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] the catalogue read reported.
pub async fn databases_like(pool: &PgPool, pattern: &str, control: ControlDatabase) -> Result<Vec<String>, sqlx::Error> {
    let mut statement = Query::select();
    statement
        .column(PgDatabase::Datname)
        .from(PgDatabase::Table)
        .and_where(Expr::col(PgDatabase::Datname).like(pattern));
    if control == ControlDatabase::Excluded {
        statement.and_where(Expr::col(PgDatabase::Datname).ne(current_database()));
    }
    statement.to_query_scalar().fetch_all(pool).await
}

/// Drop `database`, forcing off any backend still connected to it.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] the drop reported.
pub async fn force_drop(pool: &PgPool, database: &str) -> Result<(), sqlx::Error> {
    DatabaseDefinition::drop(Alias::new(database), ConnectedSessions::Terminate)
        .to_query()
        .execute(pool)
        .await?;
    Ok(())
}
