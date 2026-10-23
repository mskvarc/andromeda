//! PostgreSQL maintenance commands and inspection functions `SeaQuery` lacks.
//!
//! Relation identifiers are quoted through `SeaQuery`'s [`Iden`] contract.
//! Caller-supplied catalog values remain ordinary bound expressions.

use crate::{
    cast::cast,
    columnstore_conversion::{ChunkReference, ConvertToColumnstore, RowstoreRemoval},
    command::{Command, rendering::RenderedCommand},
    identifier::{quoted_table, quoted_tables},
    pg_type::PostgresType,
    setting::{Setting, SettingAssignment, SettingScope},
};
use sea_query::{Alias, Expr, ExprTrait, Func, FunctionCall, Iden, IntoIden, Query, QueryStatementBuilder, SimpleExpr, TableName};

/// Schema identifiers used by PostgreSQL maintenance statements.
#[derive(Clone, Copy, Iden)]
pub enum MaintenanceSchema {
    /// The ordinary application schema.
    Public,
}

/// One maintenance command on a relation, an index or a chunk.
#[derive(Clone, Debug, PartialEq)]
pub enum MaintenanceCommand {
    /// `REINDEX INDEX CONCURRENTLY schema.index`.
    ReindexIndexConcurrently(TableName),
    /// `REINDEX TABLE schema.relation`: every index of the relation rebuilt in
    /// the caller's transaction, under a `SHARE` lock on the relation and an
    /// `ACCESS EXCLUSIVE` lock on each index, all released at its end.
    ReindexTable(TableName),
    /// `DROP INDEX CONCURRENTLY schema.index`, for recovery artifacts.
    DropIndexConcurrently(TableName),
    /// `VACUUM (TRUNCATE FALSE, PARALLEL 0) schema.relation`: an ordinary
    /// vacuum with heap truncation and parallel workers disabled.
    VacuumNonTruncating(TableName),
    /// `VACUUM ANALYZE schema.relation`: an ordinary vacuum followed by a
    /// refresh of the relation's planner statistics.
    VacuumAnalyze(TableName),
    /// `ANALYZE schema.relation, ...`, which refreshes each relation's planner
    /// statistics from a bounded sample of its rows.
    Analyze(Vec<TableName>),
    /// The recompression of one partially converted `TimescaleDB` chunk,
    /// named by its OID, with truncation disabled for the call alone.
    ///
    /// `if_not_columnstore` turns a chunk recompressed meanwhile into a notice
    /// rather than an error; a partial chunk is still recompressed. With
    /// truncation disabled a write waiting on the chunk cannot deadlock the
    /// call should it fall back from recompressing segment by segment to
    /// converting the whole chunk.
    RecompressChunk(u32),
}

impl RenderedCommand for MaintenanceCommand {
    fn rendered(&self) -> String {
        match self {
            MaintenanceCommand::ReindexIndexConcurrently(index) => format!("REINDEX INDEX CONCURRENTLY {}", quoted_table(index)),
            MaintenanceCommand::ReindexTable(relation) => format!("REINDEX TABLE {}", quoted_table(relation)),
            MaintenanceCommand::DropIndexConcurrently(index) => format!("DROP INDEX CONCURRENTLY {}", quoted_table(index)),
            MaintenanceCommand::VacuumNonTruncating(relation) => format!("VACUUM (TRUNCATE FALSE, PARALLEL 0) {}", quoted_table(relation)),
            MaintenanceCommand::VacuumAnalyze(relation) => format!("VACUUM ANALYZE {}", quoted_table(relation)),
            MaintenanceCommand::Analyze(relations) => format!("ANALYZE {}", quoted_tables(relations)),
            MaintenanceCommand::RecompressChunk(oid) => ConvertToColumnstore::new(ChunkReference::Oid(*oid), RowstoreRemoval::Delete).rendered(),
        }
    }
}

impl Command for MaintenanceCommand {}

/// `schema.name` as the qualified name a maintenance command takes.
fn qualified(schema: impl IntoIden, name: impl IntoIden) -> TableName {
    TableName::from((schema.into_iden(), name.into_iden()))
}

/// `REINDEX INDEX CONCURRENTLY schema.index`.
pub fn reindex_index_concurrently(schema: impl IntoIden, index: impl IntoIden) -> MaintenanceCommand {
    MaintenanceCommand::ReindexIndexConcurrently(qualified(schema, index))
}

/// `REINDEX TABLE schema.relation`.
pub fn reindex_table(schema: impl IntoIden, relation: impl IntoIden) -> MaintenanceCommand {
    MaintenanceCommand::ReindexTable(qualified(schema, relation))
}

/// `DROP INDEX CONCURRENTLY schema.index` for recovery artifacts.
pub fn drop_index_concurrently(schema: impl IntoIden, index: impl IntoIden) -> MaintenanceCommand {
    MaintenanceCommand::DropIndexConcurrently(qualified(schema, index))
}

/// An ordinary `VACUUM` of `schema.relation` with heap truncation and
/// parallel workers disabled.
pub fn vacuum_non_truncating(schema: impl IntoIden, relation: impl IntoIden) -> MaintenanceCommand {
    MaintenanceCommand::VacuumNonTruncating(qualified(schema, relation))
}

/// `VACUUM ANALYZE schema.relation`.
pub fn vacuum_analyze(schema: impl IntoIden, relation: impl IntoIden) -> MaintenanceCommand {
    MaintenanceCommand::VacuumAnalyze(qualified(schema, relation))
}

/// `ANALYZE schema.relation`.
pub fn analyze(schema: impl IntoIden, relation: impl IntoIden) -> MaintenanceCommand {
    MaintenanceCommand::Analyze(vec![qualified(schema, relation)])
}

/// The recompression of the partially converted chunk `chunk_oid`.
#[must_use]
pub const fn recompress_chunk(chunk_oid: u32) -> MaintenanceCommand {
    MaintenanceCommand::RecompressChunk(chunk_oid)
}

/// The fixed settings for one short-lived maintenance session.
#[must_use]
pub fn session_settings() -> [SettingAssignment; 4] {
    [
        SettingAssignment::new(Setting::ApplicationName, "andromeda-postgresql-maintenance", SettingScope::Session),
        SettingAssignment::new(Setting::LockTimeout, "1s", SettingScope::Session),
        SettingAssignment::new(Setting::StatementTimeout, "30min", SettingScope::Session),
        SettingAssignment::new(Setting::MaxParallelMaintenanceWorkers, "0", SettingScope::Session),
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
pub fn storage_parameter(reloptions: SimpleExpr, parameter: Setting) -> SimpleExpr {
    let subquery = Query::select()
        .column(Alias::new("option_value"))
        .from_function(
            Func::cust(MaintenanceFunction::PgOptionsToTable).arg(reloptions),
            Alias::new("storage_parameter"),
        )
        .and_where(Expr::col(Alias::new("option_name")).eq(parameter.name_value()))
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
    use crate::{
        command::rendering::RenderedCommand,
        maintenance::{
            analyze,
            drop_index_concurrently,
            multixact_age,
            recompress_chunk,
            reindex_index_concurrently,
            reindex_table,
            session_settings,
            storage_parameter,
            transaction_age,
            vacuum_analyze,
            vacuum_non_truncating,
            wal_written_since,
        },
        setting::Setting,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};

    #[test]
    fn a_vacuum_analyze_names_the_qualified_relation() {
        assert_eq!(
            vacuum_analyze(Alias::new("public"), Alias::new("attribute_history")).rendered(),
            r#"VACUUM ANALYZE "public"."attribute_history""#
        );
    }

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
                Setting::AutovacuumVacuumThreshold,
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
        let schema = || Alias::new("public");
        let index = || Alias::new("idx_ca_value_string");
        let table = || Alias::new("current_attributes");
        assert_eq!(
            reindex_index_concurrently(schema(), index()).rendered(),
            "REINDEX INDEX CONCURRENTLY \"public\".\"idx_ca_value_string\""
        );
        assert_eq!(
            drop_index_concurrently(schema(), index()).rendered(),
            "DROP INDEX CONCURRENTLY \"public\".\"idx_ca_value_string\""
        );
        assert_eq!(
            vacuum_non_truncating(schema(), table()).rendered(),
            "VACUUM (TRUNCATE FALSE, PARALLEL 0) \"public\".\"current_attributes\""
        );
        assert_eq!(analyze(schema(), table()).rendered(), "ANALYZE \"public\".\"current_attributes\"");
        assert_eq!(
            reindex_table(Alias::new("_timescaledb_internal"), Alias::new("_hyper_1_2_chunk")).rendered(),
            "REINDEX TABLE \"_timescaledb_internal\".\"_hyper_1_2_chunk\""
        );
    }

    #[test]
    fn a_partial_chunk_is_recompressed_by_oid_with_truncation_disabled_for_the_call_alone() {
        assert_eq!(
            recompress_chunk(16_384).rendered(),
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
        let rendered = session_settings().map(|setting| setting.rendered());
        assert!(rendered.iter().any(|statement| statement.contains("lock_timeout = '1s'")));
        assert!(rendered.iter().any(|statement| statement.contains("statement_timeout = '30min'")));
        assert!(rendered.iter().any(|statement| statement.contains("max_parallel_maintenance_workers = '0'")));
    }
}
