//! PostgreSQL maintenance statements and inspection functions `SeaQuery` lacks.
//!
//! Relation identifiers are rendered through `SeaQuery`'s [`Iden`] contract.
//! Caller-supplied catalog values remain ordinary bound expressions.

use crate::{cast::cast, column_reference::quoted, pg_type::PostgresType};
use sea_query::{Alias, Expr, ExprTrait, Func, FunctionCall, Iden, Query, QueryStatementBuilder, SimpleExpr};

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

/// Build `ANALYZE schema.relation`, which refreshes the relation's planner
/// statistics from a bounded sample of its rows.
pub fn analyze(schema: &dyn Iden, relation: &dyn Iden) -> MaintenanceStatement {
    MaintenanceStatement(format!("ANALYZE {}.{}", quoted(schema), quoted(relation)))
}

/// Build the recompression of one partially converted `TimescaleDB` chunk,
/// named by its OID: `SET LOCAL timescaledb.compress_truncate_behaviour =
/// 'truncate_disabled'; CALL convert_to_columnstore(<oid>::regclass,
/// if_not_columnstore => true)`.
///
/// The two statements run as one implicit transaction, so the setting lasts for
/// the call alone. `if_not_columnstore` turns a chunk recompressed meanwhile
/// into a notice rather than an error; a partial chunk is still recompressed.
/// With truncation disabled a write waiting on the chunk cannot deadlock the
/// call should it fall back from recompressing segment by segment to converting
/// the whole chunk.
#[must_use]
pub fn recompress_chunk(chunk_oid: u32) -> MaintenanceStatement {
    MaintenanceStatement(format!(
        "SET LOCAL timescaledb.compress_truncate_behaviour = 'truncate_disabled'; CALL convert_to_columnstore({chunk_oid}::regclass, if_not_columnstore => true)"
    ))
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

/// `pg_indexes_size(relation)` physical bytes of a relation's indexes.
#[must_use]
pub fn indexes_size(relation: SimpleExpr) -> SimpleExpr {
    Func::cust(MaintenanceFunction::PgIndexesSize).args([relation]).into()
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

/// `pg_wal_lsn_diff(pg_current_wal_lsn(), lsn)::bigint`: the bytes of WAL
/// written since `lsn`, which a replication slot whose `restart_lsn` it is
/// keeps on disk.
#[must_use]
pub fn wal_written_since(lsn: SimpleExpr) -> SimpleExpr {
    let current = Func::cust(MaintenanceFunction::PgCurrentWalLsn).into();
    cast(Func::cust(MaintenanceFunction::PgWalLsnDiff).args([current, lsn]).into(), PostgresType::Bigint)
}

/// `pg_backend_pid()` for excluding the maintenance session from load gates.
#[must_use]
pub fn backend_pid() -> SimpleExpr {
    Func::cust(MaintenanceFunction::PgBackendPid).into()
}

/// `age(xid)::bigint`: how many transactions have started since `xid`.
///
/// `age` returns `integer`; the cast lets every age decode as one signed
/// 64-bit type. A frozen or special transaction ID ages as `2147483647`.
#[must_use]
pub fn transaction_age(xid: SimpleExpr) -> SimpleExpr {
    cast(Func::cust(MaintenanceFunction::Age).arg(xid).into(), PostgresType::Bigint)
}

/// `mxid_age(mxid)::bigint`: how many multixact IDs have been assigned since
/// `mxid`.
///
/// `mxid_age` returns `integer`; the cast matches [`transaction_age`].
#[must_use]
pub fn multixact_age(mxid: SimpleExpr) -> SimpleExpr {
    cast(Func::cust(MaintenanceFunction::MxidAge).arg(mxid).into(), PostgresType::Bigint)
}

/// `(SELECT option_value FROM pg_options_to_table(reloptions) WHERE
/// option_name = name)`: one storage parameter a relation sets, as text, or
/// NULL when the relation leaves it to the server setting.
///
/// `reloptions` is a `pg_class.reloptions` array; the parameter name is bound.
/// The scalar subquery yields at most one row because a relation sets each
/// storage parameter once.
#[must_use]
pub fn storage_parameter(reloptions: SimpleExpr, name: &str) -> SimpleExpr {
    let subquery = Query::select()
        .column(Alias::new("option_value"))
        .from_function(
            Func::cust(MaintenanceFunction::PgOptionsToTable).arg(reloptions),
            Alias::new("storage_parameter"),
        )
        .and_where(Expr::col(Alias::new("option_name")).eq(name.to_owned()))
        .to_owned();
    SimpleExpr::SubQuery(None, Box::new(subquery.into_sub_query_statement()))
}

/// Maintenance-related PostgreSQL functions absent from `SeaQuery`.
#[derive(Clone, Copy, Iden)]
enum MaintenanceFunction {
    /// `pg_relation_size`.
    PgRelationSize,
    /// `pg_total_relation_size`.
    PgTotalRelationSize,
    /// `pg_indexes_size`.
    PgIndexesSize,
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
    /// `age`.
    Age,
    /// `mxid_age`.
    MxidAge,
    /// `pg_options_to_table`.
    PgOptionsToTable,
    /// `pg_current_wal_lsn`.
    PgCurrentWalLsn,
    /// `pg_wal_lsn_diff`.
    PgWalLsnDiff,
}

#[cfg(test)]
mod tests {
    use crate::maintenance::{
        analyze,
        drop_index_concurrently,
        multixact_age,
        recompress_chunk,
        reindex_index_concurrently,
        session_settings,
        storage_parameter,
        transaction_age,
        vacuum_non_truncating,
        wal_written_since,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn a_transaction_age_is_read_as_a_bigint() {
        let sql = Query::select()
            .expr(transaction_age(Expr::col(Alias::new("backend_xmin"))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT age(\"backend_xmin\")::bigint");
    }

    #[test]
    fn a_multixact_age_is_read_as_a_bigint() {
        let sql = Query::select()
            .expr(multixact_age(Expr::col(Alias::new("datminmxid"))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT mxid_age(\"datminmxid\")::bigint");
    }

    #[test]
    fn a_storage_parameter_is_read_from_the_relation_options_by_bound_name() {
        let sql = Query::select()
            .expr(storage_parameter(
                Expr::col((Alias::new("c"), Alias::new("reloptions"))),
                "autovacuum_vacuum_threshold",
            ))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT (SELECT "option_value" FROM pg_options_to_table("c"."reloptions") AS "storage_parameter" WHERE "option_name" = 'autovacuum_vacuum_threshold')"#
        );
    }

    #[test]
    fn unsupported_maintenance_commands_quote_seaquery_identifiers() {
        let schema = Alias::new("public");
        let index = Alias::new("idx_ca_value_string");
        let table = Alias::new("current_attributes");
        assert_eq!(
            reindex_index_concurrently(&schema, &index).as_sql(),
            "REINDEX INDEX CONCURRENTLY \"public\".\"idx_ca_value_string\""
        );
        assert_eq!(
            drop_index_concurrently(&schema, &index).as_sql(),
            "DROP INDEX CONCURRENTLY \"public\".\"idx_ca_value_string\""
        );
        assert_eq!(
            vacuum_non_truncating(&schema, &table).as_sql(),
            "VACUUM (TRUNCATE FALSE, PARALLEL 0) \"public\".\"current_attributes\""
        );
        assert_eq!(analyze(&schema, &table).as_sql(), "ANALYZE \"public\".\"current_attributes\"");
    }

    #[test]
    fn a_partial_chunk_is_recompressed_by_oid_with_truncation_disabled_for_the_call_alone() {
        assert_eq!(
            recompress_chunk(16_384).as_sql(),
            "SET LOCAL timescaledb.compress_truncate_behaviour = 'truncate_disabled'; CALL convert_to_columnstore(16384::regclass, if_not_columnstore => true)"
        );
    }

    #[test]
    fn the_wal_a_slot_keeps_is_measured_from_the_current_position() {
        let sql = Query::select()
            .expr(wal_written_since(Expr::col(Alias::new("restart_lsn"))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT pg_wal_lsn_diff(pg_current_wal_lsn(), \"restart_lsn\")::bigint");
    }

    #[test]
    fn session_settings_pin_all_safety_limits() {
        let rendered = session_settings().map(|statement| statement.as_sql().to_owned());
        assert!(rendered.iter().any(|statement| statement.contains("lock_timeout = '1s'")));
        assert!(rendered.iter().any(|statement| statement.contains("statement_timeout = '30min'")));
        assert!(rendered.iter().any(|statement| statement.contains("max_parallel_maintenance_workers = 0")));
    }
}
