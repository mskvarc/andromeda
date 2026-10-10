//! Explaining a tenant database whose anti-wraparound vacuum is behind.
//!
//! Autovacuum freezes a database on its own, even with autovacuum disabled on
//! a table and against conflicting lock requests, so the controller never
//! freezes in its place. When a database's freeze pressure is raised, the
//! visit names what an operator needs to act on: the relations holding the
//! database's age back and the holders of the vacuum horizon, which block a
//! manual `VACUUM (FREEZE)` just as they block autovacuum.

use crate::{
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
    fleet_observation::DatabaseFreeze,
    freeze_policy::FreezePressure,
    state::RegisteredTenant,
    vacuum_horizon::horizon_holders,
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    maintenance::{multixact_age, transaction_age},
    pg_type::PostgresType,
};
use sea_query::{Alias, Expr, ExprTrait, Iden, Order, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row, postgres::PgRow};
use tracing::{error, warn};

/// How many of the oldest relations a report names.
const REPORTED_RELATIONS: u64 = 3;

/// One relation holding back its database's freeze age.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OldestRelation {
    /// Schema-qualified relation name as `regclass` prints it, for the log.
    relation: String,
    /// Age in transactions of the relation's `relfrozenxid`.
    transaction_age: u64,
    /// Age in multixact IDs of the relation's `relminmxid`.
    multixact_age: u64,
}

/// Emit one event at the level a freeze pressure warrants: a warning when
/// anti-wraparound vacuum is overdue, an error when it is critical, nothing
/// under normal pressure.
macro_rules! pressure_event {
    ($pressure:expr, $($event:tt)+) => {
        match $pressure {
            FreezePressure::Normal => {}
            FreezePressure::Overdue => warn!($($event)+),
            FreezePressure::Critical => error!($($event)+),
        }
    };
}

/// Log a raised freeze pressure with the relations and horizon holders behind
/// it. A database under normal pressure is not read.
pub(crate) async fn report_freeze_pressure(connection: &mut PgConnection, tenant: &RegisteredTenant, database: DatabaseFreeze) -> Result<(), MaintenanceError> {
    let pressure = database.pressure();
    if pressure == FreezePressure::Normal {
        return Ok(());
    }
    let relations = oldest_relations(connection).await?;
    let holders = horizon_holders(connection).await?;
    pressure_event!(pressure, tenant = tenant.log_name(), database = %tenant.database, pressure = ?pressure, transaction_age = database.transaction.age, transaction_freeze_max_age = database.transaction.freeze_max_age, multixact_age = database.multixact.age, multixact_freeze_max_age = database.multixact.freeze_max_age, horizon_holders = ?holders, "PostgreSQL anti-wraparound vacuum is behind");
    for relation in &relations {
        pressure_event!(pressure, tenant = tenant.log_name(), database = %tenant.database, pressure = ?pressure, relation = relation.relation.as_str(), transaction_age = relation.transaction_age, multixact_age = relation.multixact_age, "PostgreSQL relation holds back its database's freeze age");
    }
    Ok(())
}

/// Read the relations with the oldest `relfrozenxid`.
async fn oldest_relations(connection: &mut PgConnection) -> Result<Vec<OldestRelation>, MaintenanceError> {
    let (sql, values) = oldest_relations_statement().build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.iter().map(oldest_relation_of).collect()
}

/// The columns the oldest-relations statement projects.
#[derive(Clone, Copy, Iden)]
enum RelationColumn {
    /// `oid::regclass::text`.
    Relation,
    /// `age(relfrozenxid)`.
    TransactionAge,
    /// `mxid_age(relminmxid)`.
    MultixactAge,
}

/// Every table, materialized view and TOAST table in every schema, snapshot
/// branch schemas included, oldest `relfrozenxid` first. Other relation kinds
/// hold no frozen ID.
fn oldest_relations_statement() -> SelectStatement {
    Query::select()
        .expr_as(
            cast(cast(Expr::col(Alias::new("oid")), PostgresType::Regclass), PostgresType::Text),
            RelationColumn::Relation,
        )
        .expr_as(transaction_age(Expr::col(Alias::new("relfrozenxid"))), RelationColumn::TransactionAge)
        .expr_as(multixact_age(Expr::col(Alias::new("relminmxid"))), RelationColumn::MultixactAge)
        .from(Alias::new("pg_class"))
        .and_where(Expr::col(Alias::new("relkind")).is_in(["r", "m", "t"]))
        .order_by_expr(transaction_age(Expr::col(Alias::new("relfrozenxid"))), Order::Desc)
        .limit(REPORTED_RELATIONS)
        .to_owned()
}

/// Decode one relation row.
fn oldest_relation_of(row: &PgRow) -> Result<OldestRelation, MaintenanceError> {
    Ok(OldestRelation {
        relation: row.try_get(RelationColumn::Relation.to_string().as_str())?,
        transaction_age: non_negative(
            row.try_get(RelationColumn::TransactionAge.to_string().as_str())?,
            CatalogCounter::RelationFreezeAge,
        )?,
        multixact_age: non_negative(
            row.try_get(RelationColumn::MultixactAge.to_string().as_str())?,
            CatalogCounter::RelationFreezeAge,
        )?,
    })
}

#[cfg(test)]
mod tests {
    use crate::freeze_report::oldest_relations_statement;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn oldest_relations_statement_spans_every_schema() {
        let sql = oldest_relations_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "pg_class""#), "{sql}");
        assert!(sql.contains(r#""relkind" IN ('r', 'm', 't')"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY age("relfrozenxid")::bigint DESC LIMIT 3"#), "{sql}");
        assert!(!sql.contains("public"), "every schema is read: {sql}");
        assert!(!sql.contains("relnamespace"), "every schema is read: {sql}");
    }
}
