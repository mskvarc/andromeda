//! The `SeaQuery` statements the tenant registry runs against the `tenants` table.

use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
use andromeda_postgresql_schema::tenancy::Tenants;
use andromeda_postgresql_seaquery::{advisory_lock::advisory_xact_lock, aggregate::count_all};
use andromeda_tenancy::tenant_keyset::TenantKeyset;
use sea_query::{
    Expr,
    ExprTrait,
    InsertStatement,
    Keyword,
    OnConflict,
    Order,
    Query,
    SelectStatement,
    SimpleExpr,
    UpdateStatement,
    error::Error as SeaQueryError,
};

/// The five columns a tenant registry row projects, in the order the row mapper
/// reads them.
fn record_columns(query: &mut SelectStatement) {
    query.columns([Tenants::Name, Tenants::Status, Tenants::DatabaseName, Tenants::CreatedAt, Tenants::ModifiedAt]);
}

/// The names of every named tenant, in name order.
pub(super) fn registered_tenants_query() -> SelectStatement {
    Query::select()
        .column(Tenants::Name)
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false))
        .and_where(Expr::col(Tenants::Name).is_not_null())
        .order_by(Tenants::Name, Order::Asc)
        .to_owned()
}

/// The database name of any tenant row called `name`.
///
/// Reads any row, including the unnamed default, because its one caller is
/// provisioning, which is checking whether the row it is about to insert already
/// exists.
pub(super) fn named_tenant_database_query(name: &str) -> SelectStatement {
    Query::select()
        .column(Tenants::DatabaseName)
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::Name).eq(name))
        .to_owned()
}

/// The full registry row of the named tenant called `name`.
pub(super) fn tenant_record_query(name: &str) -> SelectStatement {
    let mut query = Query::select();
    record_columns(&mut query);
    query
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::Name).eq(name))
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false))
        .to_owned()
}

/// One page of named-tenant registry rows, positioned by `keyset` and capped at
/// `limit`.
///
/// A forward or first page reads ascending by name; a backward or from-end page
/// reads descending (the caller reverses it to present ascending). The unnamed
/// default is always excluded.
pub(super) fn tenant_records_page_query(keyset: &TenantKeyset, limit: i64) -> SelectStatement {
    let mut query = Query::select();
    record_columns(&mut query);
    query
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false))
        .and_where(Expr::col(Tenants::Name).is_not_null());
    match keyset {
        TenantKeyset::Forward { after: None } => {
            query.order_by(Tenants::Name, Order::Asc);
        }
        TenantKeyset::Forward { after: Some(after) } => {
            query.and_where(Expr::col(Tenants::Name).gt(after.as_str())).order_by(Tenants::Name, Order::Asc);
        }
        TenantKeyset::Backward { before } => {
            query
                .and_where(Expr::col(Tenants::Name).lt(before.as_str()))
                .order_by(Tenants::Name, Order::Desc);
        }
        TenantKeyset::FromEnd => {
            query.order_by(Tenants::Name, Order::Desc);
        }
    }
    // A negative window bound clamps to zero rather than wrapping.
    query.limit(u64::try_from(limit).unwrap_or(0));
    query.clone()
}

/// The number of named tenants in the registry, the collection total.
pub(super) fn tenant_count_query() -> SelectStatement {
    Query::select()
        .expr(count_all())
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false))
        .and_where(Expr::col(Tenants::Name).is_not_null())
        .to_owned()
}

/// Set a named tenant's administrative status and refresh its `modified_at`.
pub(super) fn set_status_update(name: &str, status: TenantStatusCode) -> UpdateStatement {
    let mut statement = Query::update();
    statement
        .table(Tenants::Table)
        .value(Tenants::Status, status.code())
        .value(Tenants::ModifiedAt, Expr::current_timestamp())
        .and_where(Expr::col(Tenants::Name).eq(name))
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false));
    statement.clone()
}

/// Take the transaction advisory lock keyed on `name`.
pub(super) fn advisory_lock_query(name: &str) -> SelectStatement {
    Query::select().expr(advisory_xact_lock(Expr::val(name))).to_owned()
}

/// Insert the unnamed default tenant, refreshing its timestamp if it exists.
///
/// The conflict target is `database_name`, whose unique index is what keeps a restart
/// from inserting a second default row.
pub(super) fn default_tenant_upsert(database_name: &str, status: TenantStatusCode) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(Tenants::Table)
        .columns([
            Tenants::Name,
            Tenants::IsUnnamedDefault,
            Tenants::DatabaseName,
            Tenants::Status,
            Tenants::CreatedAt,
            Tenants::ModifiedAt,
        ])
        .values([
            SimpleExpr::Keyword(Keyword::Null),
            true.into(),
            database_name.into(),
            status.code().into(),
            Expr::current_timestamp(),
            Expr::current_timestamp(),
        ])?
        .on_conflict(
            OnConflict::column(Tenants::DatabaseName)
                .values([(Tenants::ModifiedAt, Expr::current_timestamp())])
                .to_owned(),
        );
    Ok(statement.clone())
}

/// Insert a named tenant.
pub(super) fn tenant_insert(name: &str, database_name: &str, status: TenantStatusCode) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(Tenants::Table)
        .columns([
            Tenants::Name,
            Tenants::IsUnnamedDefault,
            Tenants::DatabaseName,
            Tenants::Status,
            Tenants::CreatedAt,
            Tenants::ModifiedAt,
        ])
        .values([
            name.into(),
            false.into(),
            database_name.into(),
            status.code().into(),
            Expr::current_timestamp(),
            Expr::current_timestamp(),
        ])?;
    Ok(statement.clone())
}

#[cfg(test)]
mod tests {
    use crate::tenant_registry::statements::{
        advisory_lock_query,
        default_tenant_upsert,
        named_tenant_database_query,
        registered_tenants_query,
        set_status_update,
        tenant_count_query,
        tenant_insert,
        tenant_record_query,
        tenant_records_page_query,
    };
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
    use andromeda_tenancy::tenant_keyset::TenantKeyset;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, SelectStatement, UpdateStatement};

    fn select_sql(statement: &SelectStatement) -> String {
        statement.to_string(PostgresQueryBuilder)
    }

    fn update_sql(statement: &UpdateStatement) -> String {
        statement.to_string(PostgresQueryBuilder)
    }

    fn named(raw: &str) -> TenantName {
        TenantName::try_from(raw.to_string()).expect("a valid tenant name")
    }

    #[test]
    fn registered_tenants_lists_named_tenants_in_order() {
        let sql = select_sql(&registered_tenants_query());
        assert!(sql.contains("SELECT \"name\" FROM \"tenants\""), "{sql}");
        assert!(sql.contains("\"is_unnamed_default\" ="), "{sql}");
        assert!(sql.contains("\"name\" IS NOT NULL"), "{sql}");
        assert!(sql.contains("ORDER BY \"name\" ASC"), "{sql}");
    }

    #[test]
    fn the_provisioning_lookup_reads_any_row() {
        let sql = select_sql(&named_tenant_database_query("acme"));
        assert!(sql.contains("SELECT \"database_name\" FROM \"tenants\""), "{sql}");
        assert!(sql.contains("\"name\" ="), "{sql}");
        assert!(!sql.contains("is_unnamed_default"), "provisioning must not exclude the default row: {sql}");
    }

    #[test]
    fn a_record_read_projects_every_column_and_excludes_the_default() {
        let sql = select_sql(&tenant_record_query("acme"));
        for column in ["\"name\"", "\"status\"", "\"database_name\"", "\"created_at\"", "\"modified_at\""] {
            assert!(sql.contains(column), "record read must project {column}: {sql}");
        }
        assert!(sql.contains("\"name\" ="), "{sql}");
        assert!(sql.contains("\"is_unnamed_default\" ="), "a record read excludes the unnamed default: {sql}");
    }

    #[test]
    fn a_first_page_orders_by_name_ascending_with_no_bound() {
        let sql = select_sql(&tenant_records_page_query(&TenantKeyset::first_page(), 10));
        assert!(sql.contains("ORDER BY \"name\" ASC"), "{sql}");
        assert!(sql.contains("LIMIT 10"), "{sql}");
        assert!(!sql.contains("\"name\" >"), "a first page carries no cursor bound: {sql}");
    }

    #[test]
    fn a_forward_page_carries_the_greater_than_bound_ascending() {
        let sql = select_sql(&tenant_records_page_query(&TenantKeyset::Forward { after: Some(named("acme")) }, 10));
        assert!(sql.contains("\"name\" > "), "a forward page bounds by name greater-than: {sql}");
        assert!(sql.contains("ORDER BY \"name\" ASC"), "{sql}");
    }

    #[test]
    fn a_backward_page_carries_the_less_than_bound_descending() {
        let sql = select_sql(&tenant_records_page_query(&TenantKeyset::Backward { before: named("mango") }, 10));
        assert!(sql.contains("\"name\" < "), "a backward page bounds by name less-than: {sql}");
        assert!(sql.contains("ORDER BY \"name\" DESC"), "{sql}");
    }

    #[test]
    fn a_from_end_page_orders_descending_with_no_bound() {
        let sql = select_sql(&tenant_records_page_query(&TenantKeyset::FromEnd, 10));
        assert!(sql.contains("ORDER BY \"name\" DESC"), "{sql}");
        assert!(!sql.contains("\"name\" <"), "a from-end page carries no cursor bound: {sql}");
    }

    #[test]
    fn the_count_query_counts_named_tenants_only() {
        let sql = select_sql(&tenant_count_query());
        assert!(sql.contains("COUNT(*)"), "{sql}");
        assert!(sql.contains("\"is_unnamed_default\" ="), "{sql}");
        assert!(sql.contains("\"name\" IS NOT NULL"), "{sql}");
    }

    #[test]
    fn set_status_updates_the_status_and_the_modified_stamp_for_a_named_tenant() {
        let sql = update_sql(&set_status_update("acme", TenantStatusCode::Suspended));
        assert!(sql.contains("UPDATE \"tenants\" SET"), "{sql}");
        assert!(sql.contains("\"status\" ="), "{sql}");
        assert!(sql.contains("\"modified_at\" = CURRENT_TIMESTAMP"), "{sql}");
        assert!(sql.contains("\"name\" ="), "{sql}");
        assert!(
            sql.contains("\"is_unnamed_default\" ="),
            "a status write never touches the unnamed default: {sql}"
        );
    }

    #[test]
    fn the_advisory_lock_hashes_the_bound_name() {
        let (sql, values) = {
            use sea_query_sqlx::SqlxBinder;
            advisory_lock_query("acme").build_sqlx(PostgresQueryBuilder)
        };
        assert!(sql.contains("pg_advisory_xact_lock(hashtext("), "{sql}");
        assert_eq!(values.0.0.len(), 1, "the name is bound, not interpolated: {sql}");
    }

    #[test]
    fn the_default_upsert_conflicts_on_db_name_and_refreshes_the_timestamp() {
        let sql = default_tenant_upsert("andromeda", TenantStatusCode::Active)
            .expect("arity")
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("INSERT INTO \"tenants\""), "{sql}");
        assert!(sql.contains("ON CONFLICT (\"database_name\") DO UPDATE"), "{sql}");
        assert!(sql.contains("\"modified_at\" = CURRENT_TIMESTAMP"), "{sql}");
    }

    #[test]
    fn a_named_tenant_insert_is_not_the_default() {
        let sql = tenant_insert("acme", "andromeda_acme", TenantStatusCode::Active)
            .expect("arity")
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("INSERT INTO \"tenants\""), "{sql}");
        assert!(!sql.contains("ON CONFLICT"), "a named tenant insert has no upsert: {sql}");
    }
}
