//! The SQL interface of the `pg_repack` extension, which an online repack of
//! one table drives in process.
//!
//! The extension generates every statement specific to the table it repacks:
//! the primary-key type, the log table, the trigger that fills it, the copy
//! and the statements that replay the log. It reports them through its
//! `repack.tables` view, and they are opaque text the extension owns:
//! [`RepackScript`] runs one exactly as it was generated, on the simple
//! protocol. Every other call (the version, an index definition, the replay,
//! the swap, the cleanup) is a typed statement whose arguments are bound.
//!
//! The `repack` functions are written in C and take their statement arguments
//! as `cstring`; a bound value arrives as `text`, so each is cast. The
//! catalog reads around a run (what an interrupted one left, the free space a
//! repack would give back) are the `repack_catalog` module's.

use crate::{
    cast::cast,
    command::{Command, rendering::RenderedCommand},
    introspection::current_setting,
    maintenance::to_regclass,
    pg_type::PostgresType,
    setting::{Setting, SettingScope, set_config_to},
};
use sea_query::{Alias, Expr, ExprTrait, Func, Iden, Order, Query, SelectStatement, SimpleExpr};

/// How many objects one run creates for `repack_drop` to remove: the primary
/// key type, the log table, the trigger and the copy table. Each is dropped
/// with `IF EXISTS`, so the full count also cleans up after a run that
/// stopped before creating them all.
pub const REPACK_OBJECTS: i32 = 4;

/// The `repack.tables` view, one row per table the extension can repack, with
/// the statements it generated for that table.
#[derive(Clone, Copy, Debug, Iden)]
pub enum RepackTables {
    /// View name: `repack.tables`.
    #[iden = "tables"]
    Table,
    /// The table's OID.
    Relid,
    /// The OID of the index whose key the log rows identify a row by, NULL
    /// when the table has neither a primary key nor a not-null unique key.
    Pkid,
    /// `SELECT repack.create_index_type(...)`: the composite type of the key.
    CreatePktype,
    /// `SELECT repack.create_log_table(...)`: the log table.
    CreateLog,
    /// `CREATE TRIGGER repack_trigger ...`: the trigger logging each write.
    CreateTrigger,
    /// `ALTER TABLE ... ENABLE ALWAYS TRIGGER repack_trigger`.
    EnableTrigger,
    /// The tablespace the table lives in.
    TablespaceOrig,
    /// `INSERT INTO repack.table_<oid> SELECT ... FROM ONLY ...`: the copy.
    CopyData,
    /// `ALTER TABLE repack.table_<oid> ALTER ... SET STORAGE ...`, NULL when
    /// every column keeps its type's storage.
    AlterColStorage,
    /// `ALTER TABLE repack.table_<oid> DROP COLUMN ...`, NULL when the table
    /// has no dropped column.
    DropColumns,
    /// `DELETE FROM repack.log_<oid>`.
    DeleteLog,
    /// The replay's read of the next log rows.
    SqlPeek,
    /// The replay of a logged insert.
    SqlInsert,
    /// The replay of a logged delete.
    SqlDelete,
    /// The replay of a logged update.
    SqlUpdate,
    /// The removal of replayed log rows.
    SqlPop,
}

/// The `repack` schema and the extension functions a run calls.
#[derive(Clone, Copy, Iden)]
enum RepackFunction {
    /// The extension's schema.
    #[iden = "repack"]
    Schema,
    /// `repack.version()`.
    #[iden = "repack.version"]
    Version,
    /// `repack.get_order_by(index, table)`.
    #[iden = "repack.get_order_by"]
    GetOrderBy,
    /// `repack.repack_indexdef(index, table, tablespace, concurrent)`.
    #[iden = "repack.repack_indexdef"]
    RepackIndexdef,
    /// `repack.conflicted_triggers(table)`.
    #[iden = "repack.conflicted_triggers"]
    ConflictedTriggers,
    /// `repack.disable_autovacuum(relation)`.
    #[iden = "repack.disable_autovacuum"]
    DisableAutovacuum,
    /// `repack.create_table(table, tablespace)`.
    #[iden = "repack.create_table"]
    CreateTable,
    /// `repack.repack_apply(peek, insert, delete, update, pop, count)`.
    #[iden = "repack.repack_apply"]
    RepackApply,
    /// `repack.repack_swap(table)`.
    #[iden = "repack.repack_swap"]
    RepackSwap,
    /// `repack.repack_drop(table, objects)`.
    #[iden = "repack.repack_drop"]
    RepackDrop,
}

/// The `pg_index` catalog and the columns the repack statements read.
#[derive(Clone, Copy, Iden)]
enum RepackCatalog {
    /// `pg_index`.
    #[iden = "pg_index"]
    PgIndex,
    /// `pg_index.indexrelid`.
    Indexrelid,
    /// `pg_index.indrelid`.
    Indrelid,
    /// `pg_index.indisvalid`.
    Indisvalid,
}

/// The name of the trigger a run installs on the table it repacks.
pub const REPACK_TRIGGER: &str = "repack_trigger";

/// A table's OID as a bound `oid`.
pub(crate) fn oid(relid: u32) -> SimpleExpr {
    cast(Expr::val(i64::from(relid)), PostgresType::Oid)
}

/// A bound text value as the `cstring` a C function takes.
fn cstring(text: &str) -> SimpleExpr {
    cast(Expr::val(text), PostgresType::Cstring)
}

/// `SELECT repack.version()`: the version of the extension's loaded library,
/// as `pg_repack <major>.<minor>.<patch>`.
#[must_use]
pub fn repack_version() -> SelectStatement {
    Query::select().expr(Func::cust(RepackFunction::Version)).to_owned()
}

/// The statements the extension generated for the table `relid`, its key
/// index's OID read as a `bigint`.
#[must_use]
pub fn table_script(relid: u32) -> SelectStatement {
    Query::select()
        .expr_as(cast(Expr::col(RepackTables::Pkid), PostgresType::Bigint), RepackTables::Pkid)
        .columns([
            RepackTables::CreatePktype,
            RepackTables::CreateLog,
            RepackTables::CreateTrigger,
            RepackTables::EnableTrigger,
            RepackTables::TablespaceOrig,
            RepackTables::CopyData,
            RepackTables::AlterColStorage,
            RepackTables::DropColumns,
            RepackTables::DeleteLog,
            RepackTables::SqlPeek,
            RepackTables::SqlInsert,
            RepackTables::SqlDelete,
            RepackTables::SqlUpdate,
            RepackTables::SqlPop,
        ])
        .from((RepackFunction::Schema, RepackTables::Table))
        .and_where(Expr::col(RepackTables::Relid).eq(oid(relid)))
        .to_owned()
}

/// `SELECT repack.get_order_by(to_regclass(index)::oid, relid)`: the `ORDER
/// BY` list that copies the table `relid` in the order of `index`.
#[must_use]
pub fn order_by_index(index: &str, relid: u32) -> SelectStatement {
    let index = cast(to_regclass(Expr::val(index)), PostgresType::Oid);
    Query::select()
        .expr(Func::cust(RepackFunction::GetOrderBy).args([index, oid(relid)]))
        .to_owned()
}

/// Whether the table `relid` has an invalid index, which a copy would not
/// rebuild.
#[must_use]
pub fn invalid_index_present(relid: u32) -> SelectStatement {
    let invalid = Query::select()
        .expr(Expr::val(1))
        .from(RepackCatalog::PgIndex)
        .and_where(Expr::col(RepackCatalog::Indrelid).eq(oid(relid)))
        .and_where(Expr::col(RepackCatalog::Indisvalid).not())
        .to_owned();
    Query::select().expr(Expr::exists(invalid)).to_owned()
}

/// The definition of each valid index of the table `relid`, rewritten by the
/// extension to build the same index on the copy, in index OID order.
#[must_use]
pub fn index_definitions(relid: u32) -> SelectStatement {
    let definition = Func::cust(RepackFunction::RepackIndexdef).args([
        Expr::col(RepackCatalog::Indexrelid),
        Expr::col(RepackCatalog::Indrelid),
        cast(Expr::cust("NULL"), PostgresType::Name),
        Expr::val(false),
    ]);
    Query::select()
        .expr(definition)
        .from(RepackCatalog::PgIndex)
        .and_where(Expr::col(RepackCatalog::Indrelid).eq(oid(relid)))
        .and_where(Expr::col(RepackCatalog::Indisvalid))
        .order_by(RepackCatalog::Indexrelid, Order::Asc)
        .to_owned()
}

/// Whether the table `relid` already carries a trigger named as the one a run
/// installs.
#[must_use]
pub fn conflicted_triggers(relid: u32) -> SelectStatement {
    let conflicted = Query::select()
        .expr(Expr::val(1))
        .from_function(Func::cust(RepackFunction::ConflictedTriggers).arg(oid(relid)), Alias::new("conflicted"))
        .to_owned();
    Query::select().expr(Expr::exists(conflicted)).to_owned()
}

/// `SELECT repack.disable_autovacuum(to_regclass(relation))`: keep autovacuum
/// off a relation the run owns.
#[must_use]
pub fn disable_autovacuum(relation: &str) -> SelectStatement {
    Query::select()
        .expr(Func::cust(RepackFunction::DisableAutovacuum).arg(to_regclass(Expr::val(relation))))
        .to_owned()
}

/// `SELECT repack.create_table(relid, tablespace)`: the empty copy table.
#[must_use]
pub fn create_table(relid: u32, tablespace: &str) -> SelectStatement {
    Query::select()
        .expr(Func::cust(RepackFunction::CreateTable).args([oid(relid), cast(Expr::val(tablespace), PostgresType::Name)]))
        .to_owned()
}

/// `SELECT set_config('work_mem', current_setting('maintenance_work_mem'),
/// true)`: the copy sorts with the memory a maintenance operation may use.
#[must_use]
pub fn copy_work_mem() -> SelectStatement {
    Query::select()
        .expr(set_config_to(
            Setting::WorkMem,
            current_setting(Setting::MaintenanceWorkMem),
            SettingScope::Transaction,
        ))
        .to_owned()
}

/// The statements one table's log is replayed with, as the extension
/// generated them.
#[derive(Clone, Copy, Debug)]
pub struct ReplayStatements<'s> {
    /// The read of the next log rows.
    pub peek: &'s str,
    /// The replay of a logged insert.
    pub insert: &'s str,
    /// The replay of a logged delete.
    pub delete: &'s str,
    /// The replay of a logged update.
    pub update: &'s str,
    /// The removal of replayed log rows.
    pub pop: &'s str,
}

/// `SELECT repack.repack_apply(...)`: replay at most `count` log rows onto
/// the copy, every row when `count` is zero, answering how many it replayed.
#[must_use]
pub fn apply_log(statements: ReplayStatements<'_>, count: i32) -> SelectStatement {
    Query::select()
        .expr(Func::cust(RepackFunction::RepackApply).args([
            cstring(statements.peek),
            cstring(statements.insert),
            cstring(statements.delete),
            cstring(statements.update),
            cstring(statements.pop),
            Expr::val(count),
        ]))
        .to_owned()
}

/// `SELECT repack.repack_swap(relid)`: give the table `relid` the copy's
/// files and indexes, under `ACCESS EXCLUSIVE` on both.
#[must_use]
pub fn swap(relid: u32) -> SelectStatement {
    Query::select().expr(Func::cust(RepackFunction::RepackSwap).arg(oid(relid))).to_owned()
}

/// `SELECT repack.repack_drop(relid, 4)`: drop every object a run created for
/// the table `relid`, the copy table included, which after a swap holds the
/// old files.
#[must_use]
pub fn drop_objects(relid: u32) -> SelectStatement {
    Query::select()
        .expr(Func::cust(RepackFunction::RepackDrop).args([oid(relid), Expr::val(REPACK_OBJECTS)]))
        .to_owned()
}

/// One statement the extension generated for a table, run as it was
/// generated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepackScript(String);

impl RepackScript {
    /// The statement the extension generated.
    #[must_use]
    pub const fn generated(text: String) -> RepackScript {
        RepackScript(text)
    }

    /// The generated copy `copy`, reading the rows in the generated `order`.
    #[must_use]
    pub fn ordered_copy(copy: &str, order: &str) -> RepackScript {
        RepackScript(format!("{copy} ORDER BY {order}"))
    }
}

impl RenderedCommand for RepackScript {
    fn rendered(&self) -> String {
        // The rendering contract hands the driver an owned text, and a script
        // may run more than once (a bounded lock attempt is retried).
        self.0.clone()
    }
}

impl Command for RepackScript {}

#[cfg(test)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        repack::{
            RepackScript,
            ReplayStatements,
            apply_log,
            conflicted_triggers,
            copy_work_mem,
            create_table,
            disable_autovacuum,
            drop_objects,
            index_definitions,
            invalid_index_present,
            order_by_index,
            repack_version,
            swap,
            table_script,
        },
    };
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_version_is_read_from_the_loaded_library() {
        assert_eq!(repack_version().to_string(PostgresQueryBuilder), "SELECT repack.version()");
    }

    #[test]
    fn the_generated_statements_are_read_for_one_table_by_oid() {
        let sql = table_script(16_384).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(
                r#"SELECT "pkid"::bigint AS "pkid", "create_pktype", "create_log", "create_trigger", "enable_trigger", "tablespace_orig", "copy_data""#
            ),
            "{sql}"
        );
        assert!(sql.ends_with(r#"FROM "repack"."tables" WHERE "relid" = (16384::oid)"#), "{sql}");
    }

    #[test]
    fn a_clustered_copy_orders_by_the_index_it_names() {
        assert_eq!(
            order_by_index("snap_7.pk_ca_grain", 16_384).to_string(PostgresQueryBuilder),
            "SELECT repack.get_order_by(to_regclass('snap_7.pk_ca_grain')::oid, 16384::oid)"
        );
    }

    #[test]
    fn every_valid_index_is_redefined_on_the_copy_in_oid_order() {
        assert_eq!(
            index_definitions(16_384).to_string(PostgresQueryBuilder),
            r#"SELECT repack.repack_indexdef("indexrelid", "indrelid", NULL::name, FALSE) FROM "pg_index" WHERE "indrelid" = (16384::oid) AND "indisvalid" ORDER BY "indexrelid" ASC"#
        );
        assert_eq!(
            invalid_index_present(16_384).to_string(PostgresQueryBuilder),
            r#"SELECT EXISTS(SELECT 1 FROM "pg_index" WHERE "indrelid" = (16384::oid) AND (NOT "indisvalid"))"#
        );
    }

    #[test]
    fn a_conflicting_trigger_is_asked_of_the_extension() {
        assert_eq!(
            conflicted_triggers(16_384).to_string(PostgresQueryBuilder),
            r#"SELECT EXISTS(SELECT 1 FROM repack.conflicted_triggers(16384::oid) AS "conflicted")"#
        );
    }

    #[test]
    fn the_copy_table_is_created_in_the_tables_tablespace_with_autovacuum_off() {
        assert_eq!(
            create_table(16_384, "pg_default").to_string(PostgresQueryBuilder),
            "SELECT repack.create_table(16384::oid, 'pg_default'::name)"
        );
        assert_eq!(
            disable_autovacuum("repack.table_16384").to_string(PostgresQueryBuilder),
            "SELECT repack.disable_autovacuum(to_regclass('repack.table_16384'))"
        );
        assert_eq!(
            copy_work_mem().to_string(PostgresQueryBuilder),
            "SELECT set_config('work_mem', current_setting('maintenance_work_mem'), TRUE)"
        );
    }

    #[test]
    fn the_replay_passes_each_generated_statement_as_a_cstring() {
        let statements = ReplayStatements {
            peek: "SELECT 1",
            insert: "INSERT 1",
            delete: "DELETE 1",
            update: "UPDATE 1",
            pop: "DELETE FROM repack.log_16384 WHERE id IN (",
        };
        assert_eq!(
            apply_log(statements, 1_000).to_string(PostgresQueryBuilder),
            "SELECT repack.repack_apply('SELECT 1'::cstring, 'INSERT 1'::cstring, 'DELETE 1'::cstring, 'UPDATE 1'::cstring, 'DELETE FROM repack.log_16384 WHERE id IN ('::cstring, 1000)"
        );
    }

    #[test]
    fn the_swap_and_the_cleanup_name_the_table_by_oid() {
        assert_eq!(swap(16_384).to_string(PostgresQueryBuilder), "SELECT repack.repack_swap(16384::oid)");
        assert_eq!(drop_objects(16_384).to_string(PostgresQueryBuilder), "SELECT repack.repack_drop(16384::oid, 4)");
    }

    #[test]
    fn a_generated_statement_runs_as_it_was_generated() {
        assert_eq!(
            RepackScript::generated("DELETE FROM repack.log_16384".to_owned()).rendered(),
            "DELETE FROM repack.log_16384"
        );
        assert_eq!(
            RepackScript::ordered_copy("INSERT INTO repack.table_16384 SELECT a FROM ONLY public.t", "a").rendered(),
            "INSERT INTO repack.table_16384 SELECT a FROM ONLY public.t ORDER BY a"
        );
    }
}
