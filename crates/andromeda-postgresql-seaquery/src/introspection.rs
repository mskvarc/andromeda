//! PostgreSQL server and extension introspection sea-query has no form for.
//!
//! Reading the running server's version, a loaded extension's version, or a
//! runtime setting means calling system functions (`current_setting`) and
//! querying system catalogs (`pg_extension`) that are not part of any
//! application schema. Those calls are spelled here, in the one crate
//! sanctioned to write a hand-formed fragment, with every catalog name and
//! setting name passed as a bound value or a named iden rather than
//! interpolated into a statement string.

use crate::setting::Setting;
use sea_query::{Alias, Expr, ExprTrait, Func, Iden, QueryStatementBuilder, SelectStatement, SimpleExpr};

/// The `pg_extension` system catalog.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_extension"]
struct PgExtension;

/// The `pg_available_extensions` system view of the extensions the server's
/// installation ships.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_available_extensions"]
struct PgAvailableExtensions;

/// `current_setting(name)` - the value of a runtime configuration setting.
///
/// The setting name is bound rather than interpolated. This form errors if the
/// setting is unknown; use [`current_setting_missing_ok`] for settings that may
/// be absent.
#[must_use]
pub fn current_setting(setting: Setting) -> SimpleExpr {
    Func::cust(CurrentSetting).args([setting.name_value()]).into()
}

/// `current_setting(name, true)` - the value of a runtime setting, or `NULL`
/// when the setting is not defined.
///
/// The `missing_ok` argument spares the caller a failed statement for an
/// optional setting, such as a license marker an extension only sets when
/// loaded.
#[must_use]
pub fn current_setting_missing_ok(setting: Setting) -> SimpleExpr {
    Func::cust(CurrentSetting)
        .args([setting.name_value(), SimpleExpr::from(Expr::val(true))])
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

/// `(SELECT default_version FROM pg_available_extensions WHERE name = name)` -
/// the version of one extension the server's installation ships, which
/// `CREATE EXTENSION` installs and `ALTER EXTENSION ... UPDATE` updates to, or
/// `NULL` when the installation does not ship it.
///
/// The extension name is bound. The scalar subquery yields a single row because
/// the view lists one row per extension control file.
#[must_use]
pub fn extension_shipped_version(name: &str) -> SimpleExpr {
    let subquery = SelectStatement::new()
        .expr(Expr::col(Alias::new("default_version")))
        .from(PgAvailableExtensions)
        .and_where(Expr::col(Alias::new("name")).eq(name.to_owned()))
        .to_owned();
    SimpleExpr::SubQuery(None, Box::new(subquery.into_sub_query_statement()))
}

/// `(SELECT rolsuper FROM pg_roles WHERE rolname = current_user)` - whether
/// the session's role is a superuser.
#[must_use]
pub fn current_role_is_superuser() -> SimpleExpr {
    let subquery = SelectStatement::new()
        .expr(Expr::col(Alias::new("rolsuper")))
        .from(Alias::new("pg_roles"))
        .and_where(Expr::col(Alias::new("rolname")).eq(Expr::cust("current_user")))
        .to_owned();
    SimpleExpr::SubQuery(None, Box::new(subquery.into_sub_query_statement()))
}

/// `(SELECT pg_has_role(current_user, oid, 'MEMBER') FROM pg_roles WHERE
/// rolname = role)` - whether the session's role is a member of `role`,
/// directly or through another role, or `NULL` when no role of that name
/// exists on this server.
///
/// The role is looked up by name first, so a predefined role a server release
/// does not have reads as `NULL` rather than failing the statement.
#[must_use]
pub fn current_role_is_member_of(role: &str) -> SimpleExpr {
    let subquery = SelectStatement::new()
        .expr(Func::cust(Alias::new("pg_has_role")).args([Expr::cust("current_user"), Expr::col(Alias::new("oid")), Expr::val("MEMBER")]))
        .from(Alias::new("pg_roles"))
        .and_where(Expr::col(Alias::new("rolname")).eq(role.to_owned()))
        .to_owned();
    SimpleExpr::SubQuery(None, Box::new(subquery.into_sub_query_statement()))
}

/// `pg_table_is_visible(oid)` - whether the relation `oid` names is the one an
/// unqualified name resolves to in the session's search path.
#[must_use]
pub fn table_is_visible(oid: SimpleExpr) -> SimpleExpr {
    Func::cust(PgTableIsVisible).args([oid]).into()
}

/// The `pg_table_is_visible` system function.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_table_is_visible"]
struct PgTableIsVisible;

/// `to_regnamespace(name) IS NOT NULL` - whether a schema of that name exists.
///
/// The schema name is bound; `to_regnamespace` answers `NULL` rather than
/// failing for an absent schema.
#[must_use]
pub fn schema_exists(name: &str) -> SimpleExpr {
    SimpleExpr::from(Func::cust(ToRegnamespace).arg(Expr::val(name.to_owned()))).is_not_null()
}

/// The `to_regnamespace` system function.
#[derive(Iden, Clone, Copy)]
#[iden = "to_regnamespace"]
struct ToRegnamespace;

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

/// `current_database()` - the name of the database the session is attached to.
#[must_use]
pub fn current_database() -> SimpleExpr {
    Func::cust(CurrentDatabase).args::<[SimpleExpr; 0]>([]).into()
}

/// `current_schema()` - the first schema of the session's search path that
/// exists, the one an unqualified relation name resolves into.
#[must_use]
pub fn current_schema() -> SimpleExpr {
    Func::cust(CurrentSchema).args::<[SimpleExpr; 0]>([]).into()
}

/// The `current_schema` system function.
#[derive(Iden, Clone, Copy)]
#[iden = "current_schema"]
struct CurrentSchema;

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

#[cfg(test)]
mod tests {
    use crate::{
        introspection::{
            current_role_is_member_of,
            current_role_is_superuser,
            current_schema,
            current_setting,
            current_setting_missing_ok,
            extension_shipped_version,
            extension_version,
            transaction_has_not_written,
        },
        setting::Setting,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    #[test]
    fn the_current_schema_is_a_call_without_arguments() {
        assert_eq!(
            Query::select().expr(current_schema()).to_string(PostgresQueryBuilder),
            "SELECT current_schema()"
        );
    }

    #[test]
    fn the_session_role_is_read_from_pg_roles_by_its_own_name() {
        assert_eq!(
            Query::select().expr(current_role_is_superuser()).to_string(PostgresQueryBuilder),
            r#"SELECT (SELECT "rolsuper" FROM "pg_roles" WHERE "rolname" = (current_user))"#
        );
    }

    #[test]
    fn a_role_membership_probe_looks_the_role_up_by_bound_name_first() {
        let (sql, values) = Query::select()
            .expr(current_role_is_member_of("pg_use_reserved_connections"))
            .build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT (SELECT pg_has_role(current_user, "oid", $1) FROM "pg_roles" WHERE "rolname" = $2)"#
        );
        assert_eq!(values.0.len(), 2, "{sql}");
    }

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
            .expr_as(current_setting(Setting::ServerVersion), Alias::new("v"))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("current_setting($1)"), "{sql}");
        assert_eq!(values.0.len(), 1, "{sql}");
    }

    #[test]
    fn a_missing_setting_is_tolerated_with_the_second_argument() {
        let sql = Query::select()
            .expr_as(current_setting_missing_ok(Setting::TimescaledbLicense), Alias::new("v"))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("current_setting('timescaledb.license', TRUE)"), "{sql}");
    }

    #[test]
    fn visibility_asks_the_search_path_about_the_relation_oid() {
        use crate::introspection::table_is_visible;
        use sea_query::Expr;
        let sql = Query::select()
            .expr(table_is_visible(Expr::col(Alias::new("oid"))))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT pg_table_is_visible("oid")"#);
    }

    #[test]
    fn the_schema_probe_binds_the_schema_name() {
        use crate::introspection::schema_exists;
        let (sql, values) = Query::select().expr(schema_exists("snap_7")).build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT to_regnamespace($1) IS NOT NULL");
        assert_eq!(values.0.len(), 1);
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

    #[test]
    fn a_shipped_extension_version_reads_the_installation_by_name() {
        let (sql, values) = Query::select()
            .expr_as(extension_shipped_version("timescaledb"), Alias::new("v"))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(
            sql.contains("SELECT \"default_version\" FROM \"pg_available_extensions\" WHERE \"name\" = $1"),
            "{sql}"
        );
        assert_eq!(values.0.len(), 1, "{sql}");
    }
}
