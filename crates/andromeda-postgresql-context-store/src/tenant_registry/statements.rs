//! The `SeaQuery` statements the tenant registry runs against the `tenants` table.

use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
use andromeda_postgresql_schema::tenancy::Tenants;
use andromeda_postgresql_seaquery::{advisory_lock::advisory_xact_lock, aggregate::count_all, clock::now};
use andromeda_tenancy::tenant_page_start::TenantPageStart;
use sea_query::{
    Alias,
    DeleteStatement,
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

/// The database name and status of any tenant row called `name`.
///
/// Reads any row, including the unnamed default, because its one caller is
/// provisioning, which is checking whether the row it is about to insert already
/// exists.
pub(super) fn named_tenant_standing_query(name: &str) -> SelectStatement {
    Query::select()
        .columns([Tenants::DatabaseName, Tenants::Status])
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::Name).eq(name))
        .to_owned()
}

/// The full registry row of the named tenant called `name`, provisional or
/// established.
pub(super) fn registered_tenant_query(name: &str) -> SelectStatement {
    let mut query = Query::select();
    record_columns(&mut query);
    query
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::Name).eq(name))
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false))
        .to_owned()
}

/// The status, database and last provisioning of the named tenant called
/// `name`, with the transaction's clock beside it as `observed_at`, so the
/// age of a provisional tenant is measured on one clock.
pub(super) fn settlement_query(name: &str) -> SelectStatement {
    Query::select()
        .columns([Tenants::Status, Tenants::DatabaseName, Tenants::ModifiedAt])
        .expr_as(now(), Alias::new(OBSERVED_AT))
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::Name).eq(name))
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false))
        .to_owned()
}

/// The alias [`settlement_query`] reads the transaction's clock under.
pub(super) const OBSERVED_AT: &str = "observed_at";

/// The names of every provisional tenant, in name order.
pub(super) fn provisional_tenants_query() -> SelectStatement {
    Query::select()
        .column(Tenants::Name)
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::Status).eq(TenantStatusCode::Provisional.code()))
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false))
        .order_by(Tenants::Name, Order::Asc)
        .to_owned()
}

/// Restart a provisional tenant's time to live.
pub(super) fn provisional_touch(name: &str) -> UpdateStatement {
    provisional_update(name).value(Tenants::ModifiedAt, Expr::current_timestamp()).to_owned()
}

/// Establish a provisional tenant: it exists from now on.
pub(super) fn establish_update(name: &str) -> UpdateStatement {
    provisional_update(name)
        .value(Tenants::Status, TenantStatusCode::Active.code())
        .value(Tenants::ModifiedAt, Expr::current_timestamp())
        .to_owned()
}

/// An update of the named tenant called `name` that applies only while it is
/// provisional, so it never touches an established tenant.
fn provisional_update(name: &str) -> UpdateStatement {
    Query::update()
        .table(Tenants::Table)
        .and_where(Expr::col(Tenants::Name).eq(name))
        .and_where(Expr::col(Tenants::Status).eq(TenantStatusCode::Provisional.code()))
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false))
        .to_owned()
}

/// Remove a provisional tenant's row; an established tenant's row is never
/// removed.
pub(super) fn provisional_delete(name: &str) -> DeleteStatement {
    Query::delete()
        .from_table(Tenants::Table)
        .and_where(Expr::col(Tenants::Name).eq(name))
        .and_where(Expr::col(Tenants::Status).eq(TenantStatusCode::Provisional.code()))
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false))
        .to_owned()
}

/// One page of established named-tenant registry rows, positioned by `start`
/// and capped at `limit`.
///
/// An offset, forward or first page reads ascending by name; a backward or
/// from-end page reads descending (the caller reverses it to present
/// ascending). The unnamed default is always excluded.
pub(super) fn tenant_records_page_query(start: &TenantPageStart, limit: u32) -> SelectStatement {
    let mut query = Query::select();
    record_columns(&mut query);
    query
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false))
        .and_where(Expr::col(Tenants::Name).is_not_null())
        .and_where(Expr::col(Tenants::Status).ne(TenantStatusCode::Provisional.code()));
    match start {
        TenantPageStart::Offset(offset) => {
            query.order_by(Tenants::Name, Order::Asc).offset(u64::from(*offset));
        }
        TenantPageStart::Forward { after: None } => {
            query.order_by(Tenants::Name, Order::Asc);
        }
        TenantPageStart::Forward { after: Some(after) } => {
            query.and_where(Expr::col(Tenants::Name).gt(after.as_str())).order_by(Tenants::Name, Order::Asc);
        }
        TenantPageStart::Backward { before } => {
            query
                .and_where(Expr::col(Tenants::Name).lt(before.as_str()))
                .order_by(Tenants::Name, Order::Desc);
        }
        TenantPageStart::FromEnd => {
            query.order_by(Tenants::Name, Order::Desc);
        }
    }
    query.limit(u64::from(limit));
    query.clone()
}

/// The number of established named tenants in the registry, the collection
/// total.
pub(super) fn tenant_count_query() -> SelectStatement {
    Query::select()
        .expr(count_all())
        .from(Tenants::Table)
        .and_where(Expr::col(Tenants::IsUnnamedDefault).eq(false))
        .and_where(Expr::col(Tenants::Name).is_not_null())
        .and_where(Expr::col(Tenants::Status).ne(TenantStatusCode::Provisional.code()))
        .to_owned()
}

/// Set an established named tenant's administrative status and refresh its
/// `modified_at`; a provisional tenant is left as it is.
pub(super) fn set_status_update(name: &str, status: TenantStatusCode) -> UpdateStatement {
    let mut statement = Query::update();
    statement
        .table(Tenants::Table)
        .value(Tenants::Status, status.code())
        .value(Tenants::ModifiedAt, Expr::current_timestamp())
        .and_where(Expr::col(Tenants::Name).eq(name))
        .and_where(Expr::col(Tenants::Status).ne(TenantStatusCode::Provisional.code()))
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::tenant_registry::statements::{
        advisory_lock_query,
        default_tenant_upsert,
        establish_update,
        named_tenant_standing_query,
        provisional_delete,
        provisional_tenants_query,
        provisional_touch,
        registered_tenant_query,
        registered_tenants_query,
        set_status_update,
        settlement_query,
        tenant_count_query,
        tenant_insert,
        tenant_records_page_query,
    };
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use andromeda_postgresql_codes::tenant_status::TenantStatusCode;
    use andromeda_tenancy::tenant_page_start::TenantPageStart;
    use pretty_assertions::assert_eq;
    use sea_query::{DeleteStatement, PostgresQueryBuilder, SelectStatement, UpdateStatement};

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

    fn delete_sql(statement: &DeleteStatement) -> String {
        statement.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn the_provisioning_lookup_reads_any_row() {
        let sql = select_sql(&named_tenant_standing_query("acme"));
        assert!(sql.contains("SELECT \"database_name\", \"status\" FROM \"tenants\""), "{sql}");
        assert!(sql.contains("\"name\" ="), "{sql}");
        assert!(!sql.contains("is_unnamed_default"), "provisioning must not exclude the default row: {sql}");
    }

    #[test]
    fn a_record_read_projects_every_column_and_excludes_the_default() {
        let sql = select_sql(&registered_tenant_query("acme"));
        for column in ["\"name\"", "\"status\"", "\"database_name\"", "\"created_at\"", "\"modified_at\""] {
            assert!(sql.contains(column), "record read must project {column}: {sql}");
        }
        assert!(sql.contains("\"name\" ="), "{sql}");
        assert!(sql.contains("\"is_unnamed_default\" ="), "a record read excludes the unnamed default: {sql}");
    }

    /// Clause 5.5.10: a provisional tenant does not exist, so the collection
    /// neither lists nor counts one.
    #[test]
    fn the_collection_page_and_its_total_leave_provisional_tenants_out() {
        for sql in [
            select_sql(&tenant_records_page_query(&TenantPageStart::Forward { after: None }, 10)),
            select_sql(&tenant_count_query()),
        ] {
            assert!(sql.contains("\"status\" <> 3"), "{sql}");
        }
    }

    #[test]
    fn the_settlement_read_carries_the_transaction_clock_beside_the_last_provisioning() {
        let sql = select_sql(&settlement_query("acme"));
        assert!(sql.contains("\"status\", \"database_name\", \"modified_at\""), "{sql}");
        assert!(sql.contains("NOW() AS \"observed_at\""), "{sql}");
        assert!(sql.contains("\"is_unnamed_default\" = FALSE"), "{sql}");
    }

    #[test]
    fn the_provisional_listing_reads_provisional_named_tenants_in_order() {
        let sql = select_sql(&provisional_tenants_query());
        assert!(sql.contains("SELECT \"name\" FROM \"tenants\""), "{sql}");
        assert!(sql.contains("\"status\" = 3"), "{sql}");
        assert!(sql.contains("ORDER BY \"name\" ASC"), "{sql}");
    }

    #[test]
    fn the_touch_and_the_establishment_apply_only_to_a_provisional_tenant() {
        let touch = update_sql(&provisional_touch("acme"));
        assert!(touch.contains("SET \"modified_at\" = CURRENT_TIMESTAMP"), "{touch}");
        assert!(touch.contains("\"status\" = 3"), "{touch}");
        let establish = update_sql(&establish_update("acme"));
        assert!(establish.contains("\"status\" = 1"), "{establish}");
        assert!(establish.contains("\"modified_at\" = CURRENT_TIMESTAMP"), "{establish}");
        assert!(establish.contains("WHERE \"name\" = 'acme' AND \"status\" = 3"), "{establish}");
    }

    #[test]
    fn the_retirement_removes_only_a_provisional_row() {
        let sql = delete_sql(&provisional_delete("acme"));
        assert!(sql.contains("DELETE FROM \"tenants\""), "{sql}");
        assert!(sql.contains("\"status\" = 3"), "{sql}");
        assert!(sql.contains("\"is_unnamed_default\" = FALSE"), "{sql}");
    }

    #[test]
    fn a_first_page_orders_by_name_ascending_with_no_bound() {
        let sql = select_sql(&tenant_records_page_query(&TenantPageStart::Forward { after: None }, 10));
        assert!(sql.contains("ORDER BY \"name\" ASC"), "{sql}");
        assert!(sql.contains("LIMIT 10"), "{sql}");
        assert!(!sql.contains("\"name\" >"), "a first page carries no cursor bound: {sql}");
    }

    #[test]
    fn a_forward_page_carries_the_greater_than_bound_ascending() {
        let sql = select_sql(&tenant_records_page_query(&TenantPageStart::Forward { after: Some(named("acme")) }, 10));
        assert!(sql.contains("\"name\" > "), "a forward page bounds by name greater-than: {sql}");
        assert!(sql.contains("ORDER BY \"name\" ASC"), "{sql}");
    }

    #[test]
    fn a_backward_page_carries_the_less_than_bound_descending() {
        let sql = select_sql(&tenant_records_page_query(&TenantPageStart::Backward { before: named("mango") }, 10));
        assert!(sql.contains("\"name\" < "), "a backward page bounds by name less-than: {sql}");
        assert!(sql.contains("ORDER BY \"name\" DESC"), "{sql}");
    }

    #[test]
    fn a_from_end_page_orders_descending_with_no_bound() {
        let sql = select_sql(&tenant_records_page_query(&TenantPageStart::FromEnd, 10));
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
        assert!(sql.contains("\"status\" <> 3"), "a status write never touches a provisional tenant: {sql}");
    }

    #[test]
    fn the_advisory_lock_hashes_the_bound_name() {
        let (sql, values) = advisory_lock_query("acme").build(PostgresQueryBuilder);
        assert!(sql.contains("pg_advisory_xact_lock(hashtext("), "{sql}");
        assert_eq!(values.0.len(), 1, "the name is bound, not interpolated: {sql}");
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

    /// Clause 5.5.9.2: an offset page skips by name order and carries no bound.
    #[test]
    fn an_offset_page_orders_by_name_with_offset() {
        let sql = select_sql(&tenant_records_page_query(&TenantPageStart::Offset(20), 10));
        assert!(sql.contains("ORDER BY \"name\" ASC"), "{sql}");
        assert!(sql.contains("LIMIT 10 OFFSET 20"), "{sql}");
        assert!(
            !sql.contains("\"name\" >") && !sql.contains("\"name\" <"),
            "an offset page carries no cursor bound: {sql}"
        );
    }
}
