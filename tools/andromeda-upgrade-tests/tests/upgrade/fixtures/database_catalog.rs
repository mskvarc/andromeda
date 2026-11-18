//! What the server's catalogue and a tenant database's own bookkeeping say
//! about a database: whether it is a template, which database it is, when its
//! schema was stamped and when its migrations were applied.

use crate::fixtures::{statements::name, test_error::UpgradeTestError};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_postgresql_context_store::migration_set::tenant::TENANT_LEDGER;
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType, statement::Statement, system_catalog::PgDatabase};
use sea_query::{Expr, ExprTrait, Order, Query};
use sqlx::PgPool;

/// Whether the database `database` is marked a template and whether it admits
/// connections, `None` when the server holds no such database.
pub(crate) async fn template_flags(pool: &PgPool, database: &TenantDatabaseName) -> Result<Option<(bool, bool)>, UpgradeTestError> {
    let select = Query::select()
        .columns([PgDatabase::Datistemplate, PgDatabase::Datallowconn])
        .from(PgDatabase::Table)
        .and_where(Expr::col(PgDatabase::Datname).eq(database.as_str()))
        .to_owned();
    Ok(select.to_query_as::<(bool, bool)>().fetch_optional(pool).await?)
}

/// The object identifier of the database `database` as text, `None` when the
/// server holds no such database. A database dropped and created again under
/// the same name gets another.
pub(crate) async fn database_oid(pool: &PgPool, database: &TenantDatabaseName) -> Result<Option<String>, UpgradeTestError> {
    let select = Query::select()
        .expr(cast(Expr::col(name("oid")), PostgresType::Text))
        .from(PgDatabase::Table)
        .and_where(Expr::col(PgDatabase::Datname).eq(database.as_str()))
        .to_owned();
    Ok(select.to_query_scalar::<String>().fetch_optional(pool).await?)
}

/// The instant the schema stamp of the database `pool` reaches carries, as
/// text.
pub(crate) async fn stamp_instant(pool: &PgPool) -> Result<String, UpgradeTestError> {
    let select = Query::select()
        .expr(cast(Expr::col(name("applied_at")), PostgresType::Text))
        .from(name("schema_metadata"))
        .to_owned();
    Ok(select.to_query_scalar::<String>().fetch_one(pool).await?)
}

/// The instant the tenant ledger of the database `pool` reaches records its
/// first migration applied at, as text. A copy keeps the instant its template
/// recorded.
pub(crate) async fn first_migration_applied_at(pool: &PgPool) -> Result<String, UpgradeTestError> {
    migration_applied_at(pool, Order::Asc).await
}

/// The instant the tenant ledger of the database `pool` reaches records its
/// last migration applied at, as text.
pub(crate) async fn last_migration_applied_at(pool: &PgPool) -> Result<String, UpgradeTestError> {
    migration_applied_at(pool, Order::Desc).await
}

/// The instant the tenant ledger records the migration first in `order`
/// applied at.
async fn migration_applied_at(pool: &PgPool, order: Order) -> Result<String, UpgradeTestError> {
    let select = Query::select()
        .expr(cast(Expr::col(name("installed_on")), PostgresType::Text))
        .from(name(TENANT_LEDGER))
        .order_by(name("version"), order)
        .limit(1)
        .to_owned();
    Ok(select.to_query_scalar::<String>().fetch_one(pool).await?)
}
