//! The read side of the registration persistence port.
//!
//! Every read selects the registration rows it returns, then their
//! `registration_information` rows in one more statement, and assembles each
//! registration's `information` from those rows with the names behind their
//! keys looked up once per read.

use crate::{
    csource_registration::{
        information_read::RegistrationInformationRows,
        mapper::row_to_csr,
        registration_statements::proxied_mode,
        registration_store::{PostgresRegistrationStore, persistence_error, registration_count},
        stored_registration_error::StoredRegistrationError,
    },
    error::ContextStoreError,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, page_window::PageWindow};
use andromeda_postgresql_context_query::csr::{
    coverage::{entity_id_coverage, entity_type_coverage},
    expiry_predicate::not_expired,
    projection::attribute_name_candidate,
    registration_select::registration_select,
    statement as csr_sql,
};
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use andromeda_postgresql_schema::csr::Csr;
use andromeda_registration::{error::RegistrationError, store::page::RegistrationPage};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use sea_query::{Expr as SeaExpr, ExprTrait, Iden, Order, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, PgExecutor, Row, postgres::PgRow};
use std::{collections::HashMap, hash::BuildHasher};

/// The registrations `rows` hold, each with the `information` its
/// information rows assemble to under `names`.
fn assemble<S: BuildHasher>(
    rows: &[PgRow],
    ids: &[Identifier],
    information: &RegistrationInformationRows,
    names: &HashMap<AttributeNameKey, AttributeName, S>,
) -> Result<Vec<ContextSourceRegistration>, ContextStoreError> {
    rows.iter()
        .zip(ids)
        .map(|(row, id)| {
            let assembled = information.information_of(id, names).map_err(StoredRegistrationError::Information)?;
            row_to_csr(row, assembled)
        })
        .collect()
}

/// The `id` of each registration row.
fn identifiers(rows: &[PgRow]) -> Result<Vec<Identifier>, ContextStoreError> {
    let column = Csr::Id.to_string();
    rows.iter()
        .map(|row| {
            let raw: &str = row.try_get(column.as_str())?;
            Ok(raw.parse::<Identifier>().map_err(StoredRegistrationError::Id)?)
        })
        .collect()
}

/// The registrations `rows` hold, their information read on `executor` and
/// its names looked up in `attribute_names`.
pub(super) async fn registrations_of<'connection>(
    executor: impl PgExecutor<'connection>,
    attribute_names: &dyn NameLookup<AttributeName>,
    rows: &[PgRow],
) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
    let ids = identifiers(rows).map_err(|error| persistence_error(error))?;
    let texts: Vec<&str> = ids.iter().map(Identifier::as_str).collect();
    let information = RegistrationInformationRows::read(executor, &texts)
        .await
        .map_err(|error| persistence_error(error))?;
    let keys = information
        .held_keys()
        .map_err(|error| persistence_error(ContextStoreError::from(StoredRegistrationError::Information(error))))?;
    let names = attribute_names
        .names_of(&keys)
        .await
        .map_err(|error| persistence_error(ContextStoreError::from(error)))?;
    assemble(rows, &ids, &information, &names).map_err(|error| persistence_error(error))
}

/// Every registration due at or before `cutoff`, in identifier order, with the
/// shared projection: what the expiry sweep reads and then deletes.
pub(super) fn expired_select(cutoff: DateTime<Utc>) -> SelectStatement {
    let mut select = registration_select();
    select
        .and_where(SeaExpr::col((Csr::Table, Csr::ExpiresAt)).is_not_null())
        .and_where(SeaExpr::col((Csr::Table, Csr::ExpiresAt)).lte(cutoff))
        .order_by((Csr::Table, Csr::Id), Order::Asc);
    select
}

/// The registrations due at or before `cutoff`, read without deleting them.
pub(super) async fn find_expired(store: &PostgresRegistrationStore, cutoff: DateTime<Utc>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
    let (sql, values) = expired_select(cutoff).build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(&store.pool)
        .await
        .map_err(|error| persistence_error(error))?;
    registrations_of(&store.pool, store.attribute_names.as_ref(), &rows).await
}

/// Whether a registration with `id` exists.
pub(super) async fn exists(store: &PostgresRegistrationStore, id: &Identifier) -> Result<bool, RegistrationError> {
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
        .map_err(|error| persistence_error(error))?;
    Ok(row.0)
}

/// Read one registration by identifier.
pub(super) async fn get(store: &PostgresRegistrationStore, id: &Identifier) -> Result<Option<ContextSourceRegistration>, RegistrationError> {
    let mut select = registration_select();
    // Clause 5.9.2.4: an expired registration reads as deleted even before
    // the expiry sweep removes its row.
    select.cond_where(not_expired());
    select.and_where(SeaExpr::col(Csr::Id).eq(id.as_str()));
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let rows: Vec<PgRow> = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(&store.pool)
        .await
        .map_err(|error| persistence_error(error))?
        .into_iter()
        .collect();
    Ok(registrations_of(&store.pool, store.attribute_names.as_ref(), &rows).await?.pop())
}

/// The registration `id` as the transaction on `connection` holds it: what a
/// write in that transaction has just left, whoever commits after it.
///
/// `names` are the names behind the keys the write interned, some of them
/// minted in this transaction and so invisible to a dictionary read outside
/// it.
pub(super) async fn written(
    connection: &mut PgConnection,
    id: &Identifier,
    names: &HashMap<AttributeNameKey, AttributeName>,
) -> Result<ContextSourceRegistration, RegistrationError> {
    let mut select = registration_select();
    select.and_where(SeaExpr::col(Csr::Id).eq(id.as_str()));
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(&mut *connection)
        .await
        .map_err(|error| persistence_error(error))?;
    let information = RegistrationInformationRows::read(connection, &[id.as_str()])
        .await
        .map_err(|error| persistence_error(error))?;
    let assembled = information
        .information_of(id, names)
        .map_err(|error| persistence_error(ContextStoreError::from(StoredRegistrationError::Information(error))))?;
    row_to_csr(&row, assembled).map_err(|error| persistence_error(error))
}

/// Run a compiled discovery query: the page `window` selects, ascending by
/// identifier, and the total (clause 5.10.2).
pub(super) async fn query_compiled(
    store: &PostgresRegistrationStore,
    compiled: &CompiledCsrQuery,
    window: &PageWindow,
) -> Result<RegistrationPage, RegistrationError> {
    let built = csr_sql::build_csr_query(compiled, window).map_err(|error| persistence_error(error))?;
    let (count_sql, count_values) = built.count.build_sqlx(PostgresQueryBuilder);
    let (total,): (i64,) = sqlx::query_as_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
        .fetch_one(&store.pool)
        .await
        .map_err(|error| persistence_error(error))?;
    let (sql, values) = built.select.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(&store.pool)
        .await
        .map_err(|error| persistence_error(error))?;
    let mut csrs = registrations_of(&store.pool, store.attribute_names.as_ref(), &rows).await?;
    // Clause 5.5.9.1: a backward or from-end keyset page was read descending so
    // a single `id` index scan could serve it; it is reversed here to the
    // ascending order the client sees on every page.
    built.read_order.present(&mut csrs);
    let csrs = csrs;
    Ok(RegistrationPage {
        registrations: csrs,
        total: registration_count(total)?,
    })
}

/// The unexpired registrations that may serve an entity type and/or identifier (clause 5.9.2.4), a superset decided by the caller.
pub(super) async fn find_matching(
    store: &PostgresRegistrationStore,
    entity_type: Option<&IriBuf>,
    entity_id: Option<&Identifier>,
) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
    let mut select = registration_select();
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
        .map_err(|error| persistence_error(error))?;
    registrations_of(&store.pool, store.attribute_names.as_ref(), &rows).await
}

/// The unexpired proxied registrations that may collide with an `(entity, attributes)` footprint (clauses 4.3.6.3, 5.9.2.4), a superset decided by the caller.
pub(super) async fn find_overlapping(
    store: &PostgresRegistrationStore,
    entity_id: Option<&Identifier>,
    attributes: &[IriBuf],
) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
    let mut select = registration_select();
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
        .map_err(|error| persistence_error(error))?;
    registrations_of(&store.pool, store.attribute_names.as_ref(), &rows).await
}

/// One keyset page of unexpired registrations, ordered by identifier.
pub(super) async fn find_all_active(
    store: &PostgresRegistrationStore,
    after: Option<&Identifier>,
    limit: u32,
) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
    let mut select = registration_select();
    select.cond_where(not_expired());
    if let Some(after_id) = after {
        select.and_where(SeaExpr::col(Csr::Id).gt(after_id.as_str()));
    }
    select.order_by(Csr::Id, Order::Asc).limit(u64::from(limit));
    let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(&store.pool)
        .await
        .map_err(|error| persistence_error(error))?;
    registrations_of(&store.pool, store.attribute_names.as_ref(), &rows).await
}
