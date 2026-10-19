//! Whether a tenant's database holds information, which is what decides
//! whether a provisional tenant exists.
//!
//! Clause 5.5.10 creates a tenant implicitly when an operation creating
//! information targets it. The six operations it lists write an Entity
//! (clauses 5.6.1, 5.6.7), a Temporal Evolution (clause 5.6.11), a
//! subscription (clause 5.8.1, and clause 5.11.2 for a context source
//! subscription, held in the same table) or a context source registration
//! (clauses 5.9.2, 5.11.2), each committed in the tenant's own database, so a
//! tenant exists exactly when one of those tables holds a row on the live
//! branch.

use crate::{error::ContextStoreError, tenant_database::DatabaseUrl};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::{csr::Csr, current::Entities, subscriptions::Subscriptions, temporal::TemporalEntities};
use andromeda_tenancy::database_name::TenantDatabaseName;
use sea_query::{Expr, ExprTrait, PostgresQueryBuilder, Query, SelectStatement, TableRef};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Connection, PgConnection, error::DatabaseError};
use std::borrow::Cow;

/// What a tenant's database holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Occupancy {
    /// No information: the database holds none of the rows a creating
    /// operation writes, is not migrated yet, or does not exist.
    Empty,
    /// At least one row a creating operation writes.
    HoldsInformation,
}

/// Read what the tenant database `database` holds, over a connection of its
/// own on the direct URL that is closed before returning.
///
/// A database that does not exist, or whose schema is not migrated yet, holds
/// nothing: both are what an interrupted provisioning leaves.
pub(super) async fn occupancy(direct_url: &DatabaseUrl, database: &TenantDatabaseName) -> Result<Occupancy, ContextStoreError> {
    let url = direct_url.with_database_name(database);
    let mut conn = match PgConnection::connect(url.as_str()).await {
        Ok(conn) => conn,
        Err(error) if is_absent_sqlstate(sqlstate(&error).as_deref()) => return Ok(Occupancy::Empty),
        Err(error) => return Err(error.into()),
    };
    let (sql, values) = occupancy_query().build_sqlx(PostgresQueryBuilder);
    let read = sqlx::query_scalar_with::<_, bool, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(&mut conn)
        .await;
    conn.close().await?;
    match read {
        Ok(true) => Ok(Occupancy::HoldsInformation),
        Ok(false) => Ok(Occupancy::Empty),
        Err(error) if is_absent_sqlstate(sqlstate(&error).as_deref()) => Ok(Occupancy::Empty),
        Err(error) => Err(error.into()),
    }
}

/// The SQLSTATE a failure carries, if the database reported one.
fn sqlstate(error: &sqlx::Error) -> Option<String> {
    error.as_database_error().and_then(DatabaseError::code).map(Cow::into_owned)
}

/// Whether `code` reports a database (3D000), schema (3F000) or table (42P01)
/// that does not exist.
fn is_absent_sqlstate(code: Option<&str>) -> bool {
    code.is_some_and(|code| matches!(code, "3D000" | "3F000" | "42P01"))
}

/// Whether any table a creating operation writes holds a row, one bounded
/// probe per table.
fn occupancy_query() -> SelectStatement {
    let live = ContextTarget::Live;
    let held = holds_a_row(table_of(&live, Entities::Table))
        .or(holds_a_row(table_of(&live, TemporalEntities::Table)))
        .or(holds_a_row(table_of(&live, Subscriptions::Table)))
        .or(holds_a_row(TableRef::from(Csr::Table)));
    Query::select().expr(held).to_owned()
}

/// `EXISTS (SELECT 1 FROM table LIMIT 1)`.
fn holds_a_row(table: TableRef) -> Expr {
    Expr::exists(Query::select().expr(Expr::val(1)).from(table).limit(1).to_owned())
}

#[cfg(test)]
mod tests {
    use crate::tenant_registry::occupancy::{is_absent_sqlstate, occupancy_query};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_probe_reads_every_table_a_creating_operation_writes() {
        let sql = occupancy_query().to_string(PostgresQueryBuilder);
        for table in ["\"entities\"", "\"temporal_entities\"", "\"subscriptions\"", "\"context_source_registrations\""] {
            assert!(sql.contains(&format!("EXISTS(SELECT 1 FROM {table} LIMIT 1)")), "{table} is probed: {sql}");
        }
        assert_eq!(sql.matches(" OR ").count(), 3, "{sql}");
    }

    #[test]
    fn a_missing_database_schema_or_table_reads_as_empty() {
        for code in ["3D000", "3F000", "42P01"] {
            assert!(is_absent_sqlstate(Some(code)), "{code}");
        }
        for code in ["55006", "53300", "08006"] {
            assert!(!is_absent_sqlstate(Some(code)), "{code}");
        }
        assert!(!is_absent_sqlstate(None));
    }
}
