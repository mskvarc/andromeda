//! The read side of the registration persistence port.

use crate::csource_registration::{
    mapper::row_to_csr,
    registration_statements::{csr_base_select, proxied_mode},
    registration_store::{PostgresRegistrationStore, persistence_error, registration_count},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, pagination::Pagination};
use andromeda_postgresql_context_query::csr::{
    coverage::{entity_id_coverage, entity_type_coverage},
    expiry_predicate::not_expired,
    projection::attribute_name_candidate,
    statement as csr_sql,
};
use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::aggregate::count_all;
use andromeda_registration::{error::RegistrationError, store::RegistrationPage};
use iri_rs::IriBuf;
use sea_query::{Expr as SeaExpr, ExprTrait, Order, PostgresQueryBuilder, Query};
use sea_query_sqlx::SqlxBinder;

/// Whether a registration with `id` exists.
pub(super) async fn exists(store: &PostgresRegistrationStore, id: &Uri) -> Result<bool, RegistrationError> {
    let inner = Query::select()
        .expr(SeaExpr::val(1))
        .from(Csr::Table)
        .and_where(SeaExpr::col(Csr::Id).eq(id.as_str()))
        .to_owned();
    let statement = Query::select().expr(SeaExpr::exists(inner)).to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let row: (bool,) = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(&store.pool)
        .await
        .map_err(persistence_error)?;
    Ok(row.0)
}

/// Read one registration by identifier.
pub(super) async fn get(store: &PostgresRegistrationStore, id: &Uri) -> Result<Option<ContextSourceRegistration>, RegistrationError> {
    let mut select = csr_base_select();
    // Clause 5.9.2.4: an expired registration reads as deleted even before
    // the expiry sweep removes its row.
    select.cond_where(not_expired());
    select.and_where(SeaExpr::col(Csr::Id).eq(id.as_str()));
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(&store.pool)
        .await
        .map_err(persistence_error)?;
    match row {
        Some(row) => Ok(Some(row_to_csr(&row).map_err(persistence_error)?)),
        None => Ok(None),
    }
}

/// One deterministic page of every registration and the total count.
pub(super) async fn list(store: &PostgresRegistrationStore, pagination: Pagination) -> Result<RegistrationPage, RegistrationError> {
    let count = Query::select().expr(count_all()).from(Csr::Table).cond_where(not_expired()).to_owned();
    let (count_sql, count_values) = count.build_sqlx(PostgresQueryBuilder);
    let count_row: (i64,) = sqlx::query_as_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
        .fetch_one(&store.pool)
        .await
        .map_err(persistence_error)?;
    let mut select = csr_base_select();
    select
        .cond_where(not_expired())
        .order_by(Csr::CreatedAt, Order::Asc)
        .limit(u64::from(pagination.limit))
        .offset(u64::from(pagination.offset));
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(&store.pool)
        .await
        .map_err(persistence_error)?;
    let csrs = rows
        .iter()
        .map(|row| row_to_csr(row).map_err(persistence_error))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RegistrationPage {
        registrations: csrs,
        total: registration_count(count_row.0)?,
    })
}

/// Run a compiled discovery query: its page and its total (clause 5.10.2).
pub(super) async fn query_compiled(store: &PostgresRegistrationStore, compiled: &CompiledCsrQuery) -> Result<RegistrationPage, RegistrationError> {
    let built = csr_sql::build_csr_query(compiled).map_err(persistence_error)?;
    let (count_sql, count_values) = built.count.build_sqlx(PostgresQueryBuilder);
    let (total,): (i64,) = sqlx::query_as_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
        .fetch_one(&store.pool)
        .await
        .map_err(persistence_error)?;
    let (sql, values) = built.select.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(&store.pool)
        .await
        .map_err(persistence_error)?;
    let csrs = rows
        .iter()
        .map(|row| row_to_csr(row).map_err(persistence_error))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RegistrationPage {
        registrations: csrs,
        total: registration_count(total)?,
    })
}

/// The unexpired registrations that may serve an entity type and/or identifier (clause 5.9.2.4), a superset decided by the caller.
pub(super) async fn find_matching(
    store: &PostgresRegistrationStore,
    entity_type: Option<&IriBuf>,
    entity_id: Option<&Uri>,
) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
    let mut select = csr_base_select();
    select.cond_where(not_expired());
    if let Some(entity_type) = entity_type {
        select.and_where(entity_type_coverage(entity_type));
    }
    if let Some(entity_id) = entity_id {
        select.and_where(entity_id_coverage(entity_id));
    }
    select.order_by(Csr::CreatedAt, Order::Asc);
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(&store.pool)
        .await
        .map_err(persistence_error)?;
    rows.iter().map(|row| row_to_csr(row).map_err(persistence_error)).collect()
}

/// The unexpired proxied registrations that may collide with an `(entity, attributes)` footprint (clauses 4.3.6.3, 5.9.2.4), a superset decided by the caller.
pub(super) async fn find_overlapping(
    store: &PostgresRegistrationStore,
    entity_id: Option<&Uri>,
    attributes: &[IriBuf],
) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
    let mut select = csr_base_select();
    select.cond_where(not_expired());
    select.and_where(proxied_mode());
    if let Some(entity_id) = entity_id {
        select.and_where(entity_id_coverage(entity_id));
    }
    // The attribute preselect admits a row declaring one of the names and a
    // row declaring none at all (the `{0}` sentinel), which is exactly the
    // clause 4.3.6.3 overlap: a whole-entity registration covers every
    // attribute. The typed residual in registration policy decides.
    if !attributes.is_empty() {
        select.and_where(attribute_name_candidate(attributes));
    }
    select.order_by(Csr::CreatedAt, Order::Asc);
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(&store.pool)
        .await
        .map_err(persistence_error)?;
    rows.iter().map(|row| row_to_csr(row).map_err(persistence_error)).collect()
}

/// One keyset page of unexpired registrations, ordered by identifier.
pub(super) async fn find_all_active(
    store: &PostgresRegistrationStore,
    after: Option<&Uri>,
    limit: u32,
) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
    let mut select = csr_base_select();
    select.cond_where(not_expired());
    if let Some(after_id) = after {
        select.and_where(SeaExpr::col(Csr::Id).gt(after_id.as_str()));
    }
    select.order_by(Csr::Id, Order::Asc).limit(u64::from(limit));
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(&store.pool)
        .await
        .map_err(persistence_error)?;
    rows.iter().map(|row| row_to_csr(row).map_err(persistence_error)).collect()
}
