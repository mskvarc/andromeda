//! The platform tables, which share the platform database with the unnamed
//! default tenant.
//!
//! The platform database is also the default tenant's database, so its
//! `pg_stat_database` counters count the writes to the tenant registry, the
//! wake schedule, the tenants' extension versions, the forwarded subscription
//! routes and the maintenance controller's own state beside the tenant's. None of those is a change to
//! the tenant's context, and the controller writes its state on every pass, so
//! the fleet observation takes the platform tables' counters back out of that
//! database's. Subtracting keeps the database's own counters as the base, so a
//! dropped history chunk or Snapshot schema does not read as a statistics
//! reset.

use andromeda_postgresql_schema::{
    maintenance::{PostgresqlMaintenanceFleet, PostgresqlMaintenanceRepackRuns, PostgresqlMaintenanceTargets, PostgresqlMaintenanceTenants},
    subscription_forwarding::ForwardedSubscriptionRoutes,
    tenancy::{TenantExtensionVersions, TenantWakeSchedule, Tenants},
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    introspection::current_database,
    maintenance::to_regclass,
    pg_type::PostgresType,
    scalar_subquery::scalar_subquery,
};
use sea_query::{Alias, Expr, ExprTrait, Func, Iden, Query, SimpleExpr};

/// The name of every table the platform migrations create.
fn platform_tables() -> [String; 8] {
    [
        Tenants::Table.to_string(),
        TenantWakeSchedule::Table.to_string(),
        TenantExtensionVersions::Table.to_string(),
        ForwardedSubscriptionRoutes::Table.to_string(),
        PostgresqlMaintenanceTenants::Table.to_string(),
        PostgresqlMaintenanceTargets::Table.to_string(),
        PostgresqlMaintenanceFleet::Table.to_string(),
        PostgresqlMaintenanceRepackRuns::Table.to_string(),
    ]
}

/// `n_tup_ins + n_tup_upd + n_tup_del` summed over the platform tables when
/// `database` names the database the session is attached to, zero for every
/// other database.
///
/// `pg_stat_all_tables` reports only the attached database, which for the
/// platform connection is the platform database. The table names resolve
/// through the session's search path, as the platform migrations did.
pub(crate) fn platform_tuple_changes(database: SimpleExpr) -> SimpleExpr {
    let changes = Expr::col(Alias::new("n_tup_ins"))
        .add(Expr::col(Alias::new("n_tup_upd")))
        .add(Expr::col(Alias::new("n_tup_del")));
    let tables = platform_tables().map(|table| to_regclass(Expr::val(table)));
    let summed = Query::select()
        .expr(Func::coalesce([SimpleExpr::from(Func::sum(changes)), Expr::val(0)]))
        .from(Alias::new("pg_stat_all_tables"))
        .and_where(Expr::col(Alias::new("relid")).is_in(tables))
        .to_owned();
    Expr::case(database.eq(current_database()), cast(scalar_subquery(summed), PostgresType::Bigint))
        .finally(Expr::val(0_i64))
        .into()
}

#[cfg(test)]
mod tests {
    use crate::platform_relations::{platform_tables, platform_tuple_changes};
    use andromeda_postgresql_context_store::migration_set::platform::platform_migrations;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};
    use std::collections::BTreeSet;

    #[test]
    fn every_table_the_platform_migrations_create_is_a_platform_table() {
        let created = platform_migrations()
            .iter()
            .flat_map(|migration| migration.sql.as_str().lines())
            .filter_map(|line| {
                let line = line.trim().to_ascii_lowercase();
                let table = line.strip_prefix("create table ")?;
                let table = table.strip_prefix("if not exists ").unwrap_or(table);
                table
                    .split(|character: char| character.is_whitespace() || character == '(')
                    .next()
                    .map(str::to_owned)
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(created, platform_tables().into_iter().collect::<BTreeSet<_>>());
    }

    #[test]
    fn the_platform_tables_are_counted_only_in_the_attached_database() {
        let sql = Query::select()
            .expr(platform_tuple_changes(Expr::col(Alias::new("datname"))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"CASE WHEN ("datname" = current_database()) THEN"#), "{sql}");
        assert!(sql.contains("ELSE 0 END"), "{sql}");
        assert!(
            sql.contains(r#"SUM("n_tup_ins" + "n_tup_upd" + "n_tup_del")"#),
            "the same three counters the database total sums: {sql}"
        );
        assert!(sql.contains(r#"FROM "pg_stat_all_tables" WHERE "relid" IN (to_regclass('tenants'), "#), "{sql}");
        assert!(sql.contains("to_regclass('postgresql_maintenance_tenants')"), "{sql}");
        assert!(sql.contains("to_regclass('postgresql_maintenance_targets')"), "{sql}");
        assert!(sql.contains("to_regclass('postgresql_maintenance_fleet')"), "{sql}");
    }
}
