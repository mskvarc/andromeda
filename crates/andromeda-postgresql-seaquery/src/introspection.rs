//! PostgreSQL server and extension introspection sea-query has no form for.
//!
//! Reading the running server's version, a loaded extension's version, or a
//! runtime setting means calling system functions (`current_setting`) and
//! querying system catalogs (`pg_extension`) that are not part of any
//! application schema. Those calls are spelled here, in the one crate
//! sanctioned to write a hand-formed fragment, with every catalog name and
//! setting name passed as a bound value or a named iden rather than
//! interpolated into a statement string.

use sea_query::{Alias, Expr, ExprTrait, Func, Iden, QueryStatementBuilder, SelectStatement, SimpleExpr};

/// The `pg_extension` system catalog.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_extension"]
struct PgExtension;

/// The `pg_proc` system catalog of registered routines.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_proc"]
struct PgProc;

/// `current_setting(name)` - the value of a runtime configuration setting.
///
/// The setting name is bound rather than interpolated. This form errors if the
/// setting is unknown; use [`current_setting_missing_ok`] for settings that may
/// be absent.
#[must_use]
pub fn current_setting(name: &str) -> SimpleExpr {
    Func::cust(CurrentSetting).args([SimpleExpr::from(Expr::val(name.to_owned()))]).into()
}

/// `current_setting(name, true)` - the value of a runtime setting, or `NULL`
/// when the setting is not defined.
///
/// The `missing_ok` argument spares the caller a failed statement for an
/// optional setting, such as a license marker an extension only sets when
/// loaded.
#[must_use]
pub fn current_setting_missing_ok(name: &str) -> SimpleExpr {
    Func::cust(CurrentSetting)
        .args([SimpleExpr::from(Expr::val(name.to_owned())), SimpleExpr::from(Expr::val(true))])
        .into()
}

/// `(SELECT extversion FROM pg_extension WHERE extname = name)` - the installed
/// version of one extension, or `NULL` when it is not installed.
///
/// The extension name is bound. The scalar subquery yields a single row because
/// `pg_extension.extname` is unique.
#[must_use]
pub fn extension_version(name: &str) -> SimpleExpr {
    let subquery = SelectStatement::new()
        .expr(Expr::col(Alias::new("extversion")))
        .from(PgExtension)
        .and_where(Expr::col(Alias::new("extname")).eq(name.to_owned()))
        .to_owned();
    SimpleExpr::SubQuery(None, Box::new(subquery.into_sub_query_statement()))
}

/// `EXISTS (SELECT 1 FROM pg_proc WHERE proname = name)` - whether a routine of
/// that name is registered in the current database.
///
/// The routine name is bound. A `TimescaleDB` edition ships a TSL-only procedure
/// exactly when it is the licensed build, so probing one procedure's presence
/// distinguishes the edition without reading any license marker; that is a
/// different question, and a different catalog, from
/// [`current_setting_missing_ok`] over `timescaledb.license`.
#[must_use]
pub fn function_exists(name: &str) -> SimpleExpr {
    let subquery = SelectStatement::new()
        .expr(Expr::val(1))
        .from(PgProc)
        .and_where(Expr::col(Alias::new("proname")).eq(name.to_owned()))
        .to_owned();
    Expr::exists(subquery)
}

/// The `current_setting` system function.
#[derive(Iden, Clone, Copy)]
#[iden = "current_setting"]
struct CurrentSetting;

/// `pg_current_xact_id_if_assigned()` - the transaction id of the calling
/// transaction, or `NULL` while it has none.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_current_xact_id_if_assigned"]
struct PgCurrentXactIdIfAssigned;

/// `pg_current_xact_id_if_assigned() IS NULL` - whether the calling
/// transaction has written nothing yet.
///
/// A transaction is assigned an id by its first write, so a null answer means
/// every row the transaction's snapshot shows was committed by someone else;
/// a read that asks this alongside its rows can tell a committed hit from one
/// the transaction itself may still roll back.
#[must_use]
pub fn transaction_has_not_written() -> SimpleExpr {
    SimpleExpr::from(Func::cust(PgCurrentXactIdIfAssigned)).is_null()
}

/// `pg_column_size(to_jsonb(row))` - deterministic logical size of one relation row.
#[must_use]
pub fn jsonb_row_size(row: SimpleExpr) -> SimpleExpr {
    let json = SimpleExpr::from(Func::cust(ToJsonb).arg(row));
    Func::cust(PgColumnSize).arg(json).into()
}

/// `current_database()` - the name of the database the session is attached to.
#[must_use]
pub fn current_database() -> SimpleExpr {
    Func::cust(CurrentDatabase).args::<[SimpleExpr; 0]>([]).into()
}

/// `pg_database_size(current_database())` - the total on-disk size, in bytes, of
/// the database the session is attached to.
///
/// `pg_database_size` already returns `bigint`, so no cast is spelled.
#[must_use]
pub fn current_database_size() -> SimpleExpr {
    Func::cust(PgDatabaseSize).arg(current_database()).into()
}

/// The `current_database` system function.
#[derive(Iden, Clone, Copy)]
#[iden = "current_database"]
struct CurrentDatabase;

/// The `pg_database_size` system sizing function.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_database_size"]
struct PgDatabaseSize;

/// The `to_jsonb` PostgreSQL conversion function.
#[derive(Iden, Clone, Copy)]
#[iden = "to_jsonb"]
struct ToJsonb;

/// The `pg_column_size` PostgreSQL sizing function.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_column_size"]
struct PgColumnSize;

#[cfg(test)]
mod tests {
    use crate::introspection::{current_setting, current_setting_missing_ok, extension_version, function_exists, transaction_has_not_written};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    #[test]
    fn the_unwritten_transaction_test_asks_for_a_null_transaction_id() {
        let (sql, values) = Query::select()
            .expr_as(transaction_has_not_written(), Alias::new("fresh"))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("pg_current_xact_id_if_assigned() IS NULL"), "{sql}");
        assert!(values.0.is_empty(), "{sql}");
    }

    #[test]
    fn current_setting_binds_the_setting_name() {
        let (sql, values) = Query::select()
            .expr_as(current_setting("server_version"), Alias::new("v"))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("current_setting($1)"), "{sql}");
        assert_eq!(values.0.len(), 1, "{sql}");
    }

    #[test]
    fn a_missing_setting_is_tolerated_with_the_second_argument() {
        let sql = Query::select()
            .expr_as(current_setting_missing_ok("timescaledb.license"), Alias::new("v"))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("current_setting('timescaledb.license', TRUE)"), "{sql}");
    }

    #[test]
    fn a_function_presence_probe_binds_the_routine_name_over_pg_proc() {
        let sql = Query::select()
            .expr_as(function_exists("add_columnstore_policy"), Alias::new("present"))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(
            sql.contains("EXISTS(SELECT 1 FROM \"pg_proc\" WHERE \"proname\" = 'add_columnstore_policy')"),
            "{sql}"
        );
    }

    #[test]
    fn the_database_size_sizes_the_current_database() {
        use crate::introspection::current_database_size;
        let sql = Query::select()
            .expr_as(current_database_size(), Alias::new("bytes"))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("pg_database_size(current_database())"), "{sql}");
    }

    #[test]
    fn an_extension_version_reads_the_catalog_by_name() {
        let sql = Query::select()
            .expr_as(extension_version("postgis"), Alias::new("v"))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("SELECT \"extversion\" FROM \"pg_extension\""), "{sql}");
        assert!(sql.contains("\"extname\" = 'postgis'"), "{sql}");
    }
}
