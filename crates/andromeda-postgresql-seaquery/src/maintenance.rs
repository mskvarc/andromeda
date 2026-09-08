//! PostgreSQL maintenance statements and inspection functions `SeaQuery` lacks.
//!
//! Relation identifiers are rendered through `SeaQuery`'s [`Iden`] contract.
//! Caller-supplied catalog values remain ordinary bound expressions.

use crate::column_reference::quoted;
use sea_query::{Expr, Func, FunctionCall, Iden, SimpleExpr};

/// Schema identifiers used by PostgreSQL maintenance statements.
#[derive(Clone, Copy, Iden)]
pub enum MaintenanceSchema {
    /// The ordinary application schema.
    Public,
}

/// A fully rendered PostgreSQL statement whose identifiers were `SeaQuery` idens.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaintenanceStatement(String);

impl MaintenanceStatement {
    /// Borrow the rendered SQL for execution through the database driver.
    #[must_use]
    pub fn as_sql(&self) -> &str {
        &self.0
    }
}

/// Build `REINDEX INDEX CONCURRENTLY schema.index`.
pub fn reindex_index_concurrently(schema: &dyn Iden, index: &dyn Iden) -> MaintenanceStatement {
    MaintenanceStatement(format!("REINDEX INDEX CONCURRENTLY {}.{}", quoted(schema), quoted(index)))
}

/// Build `DROP INDEX CONCURRENTLY schema.index` for recovery artifacts.
pub fn drop_index_concurrently(schema: &dyn Iden, index: &dyn Iden) -> MaintenanceStatement {
    MaintenanceStatement(format!("DROP INDEX CONCURRENTLY {}.{}", quoted(schema), quoted(index)))
}

/// Build ordinary `VACUUM` with heap truncation and parallel workers disabled.
pub fn vacuum_non_truncating(schema: &dyn Iden, relation: &dyn Iden) -> MaintenanceStatement {
    MaintenanceStatement(format!("VACUUM (TRUNCATE FALSE, PARALLEL 0) {}.{}", quoted(schema), quoted(relation)))
}

/// Build the fixed settings for one short-lived maintenance session.
#[must_use]
pub fn session_settings() -> [MaintenanceStatement; 4] {
    [
        MaintenanceStatement("SET application_name = 'andromeda-postgresql-maintenance'".to_owned()),
        MaintenanceStatement("SET lock_timeout = '1s'".to_owned()),
        MaintenanceStatement("SET statement_timeout = '30min'".to_owned()),
        MaintenanceStatement("SET max_parallel_maintenance_workers = 0".to_owned()),
    ]
}

/// `pg_relation_size(relation)` physical bytes.
#[must_use]
pub fn relation_size(relation: SimpleExpr) -> SimpleExpr {
    Func::cust(MaintenanceFunction::PgRelationSize).args([relation]).into()
}

/// `pg_total_relation_size(relation)` physical bytes including owned forks.
#[must_use]
pub fn total_relation_size(relation: SimpleExpr) -> SimpleExpr {
    Func::cust(MaintenanceFunction::PgTotalRelationSize).args([relation]).into()
}

/// `to_regclass(name)` resolving a bound qualified relation name.
#[must_use]
pub fn to_regclass(name: SimpleExpr) -> SimpleExpr {
    Func::cust(MaintenanceFunction::ToRegclass).args([name]).into()
}

/// `to_regprocedure(signature)` resolving a bound function signature.
#[must_use]
pub fn to_regprocedure(signature: SimpleExpr) -> SimpleExpr {
    Func::cust(MaintenanceFunction::ToRegprocedure).args([signature]).into()
}

/// `has_function_privilege(current_user, function, privilege)`.
#[must_use]
pub fn current_user_has_function_privilege(function: SimpleExpr, privilege: SimpleExpr) -> SimpleExpr {
    Func::cust(MaintenanceFunction::HasFunctionPrivilege)
        .args([Expr::cust("current_user"), function, privilege])
        .into()
}

/// `has_table_privilege(current_user, relation, privilege)`.
#[must_use]
pub fn current_user_has_table_privilege(relation: SimpleExpr, privilege: SimpleExpr) -> SimpleExpr {
    Func::cust(MaintenanceFunction::HasTablePrivilege)
        .args([Expr::cust("current_user"), relation, privilege])
        .into()
}

/// `pgstattuple(relation)` as a table-valued function source.
#[must_use]
pub fn pgstattuple(relation: SimpleExpr) -> FunctionCall {
    Func::cust(MaintenanceFunction::Pgstattuple).args([relation])
}

/// `pg_backend_pid()` for excluding the maintenance session from load gates.
#[must_use]
pub fn backend_pid() -> SimpleExpr {
    Func::cust(MaintenanceFunction::PgBackendPid).into()
}

/// `EXTRACT(EPOCH FROM clock_timestamp() - value)`.
#[must_use]
pub fn age_seconds(value: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("EXTRACT(EPOCH FROM (clock_timestamp() - $1))", [value])
}

/// Maintenance-related PostgreSQL functions absent from `SeaQuery`.
#[derive(Clone, Copy, Iden)]
enum MaintenanceFunction {
    /// `pg_relation_size`.
    PgRelationSize,
    /// `pg_total_relation_size`.
    PgTotalRelationSize,
    /// `to_regclass`.
    ToRegclass,
    /// `to_regprocedure`.
    ToRegprocedure,
    /// `has_function_privilege`.
    HasFunctionPrivilege,
    /// `has_table_privilege`.
    HasTablePrivilege,
    /// `pgstattuple`.
    Pgstattuple,
    /// `pg_backend_pid`.
    PgBackendPid,
}

#[cfg(test)]
mod tests {
    use crate::maintenance::{drop_index_concurrently, reindex_index_concurrently, session_settings, vacuum_non_truncating};
    use sea_query::Alias;

    #[test]
    fn unsupported_maintenance_commands_quote_seaquery_identifiers() {
        let schema = Alias::new("public");
        let index = Alias::new("idx_cav_attr_string");
        let table = Alias::new("current_attribute_values");
        assert_eq!(
            reindex_index_concurrently(&schema, &index).as_sql(),
            "REINDEX INDEX CONCURRENTLY \"public\".\"idx_cav_attr_string\""
        );
        assert_eq!(
            drop_index_concurrently(&schema, &index).as_sql(),
            "DROP INDEX CONCURRENTLY \"public\".\"idx_cav_attr_string\""
        );
        assert_eq!(
            vacuum_non_truncating(&schema, &table).as_sql(),
            "VACUUM (TRUNCATE FALSE, PARALLEL 0) \"public\".\"current_attribute_values\""
        );
    }

    #[test]
    fn session_settings_pin_all_safety_limits() {
        let rendered = session_settings().map(|statement| statement.as_sql().to_owned());
        assert!(rendered.iter().any(|statement| statement.contains("lock_timeout = '1s'")));
        assert!(rendered.iter().any(|statement| statement.contains("statement_timeout = '30min'")));
        assert!(rendered.iter().any(|statement| statement.contains("max_parallel_maintenance_workers = 0")));
    }
}
