//! PostgreSQL implementation of the registration persistence port.

use crate::csource_registration::{mapper::row_to_csr, routing_rewrite::replace_registration_information};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    context_source_pseudonym::ContextSourcePseudonym,
    non_empty_text::NonEmptyText,
    scope::grammar::ScopeBuf,
    tenant::TenantName,
    uri::Uri,
};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, CsrOperation, CsrStatus, ForwardingMode};
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, dataset::DatasetSelector, pagination::Pagination};
use andromeda_postgresql_context_query::csr::{
    coverage::{entity_id_coverage, entity_type_coverage},
    projection::attribute_name_candidate,
    statement as csr_sql,
};
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_schema::csr::{Csr, RegistrationInformation};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    clock::now,
    literal::text_literal,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    postgis::{geometry_as_geojson, validated_geom_from_ewkb},
};
use andromeda_registration::{
    error::RegistrationError,
    resolved_update::ResolvedCsrUpdate,
    store::{CreateCsrParams, ForwardingOutcome, RegistrationMutation, RegistrationPage, RegistrationStore},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use sea_query::{CaseStatement, Cond, Expr as SeaExpr, ExprTrait, Keyword, Order, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::postgres::PgPool;
use std::{fmt::Display, sync::Arc};

/// PostgreSQL-backed registration persistence adapter.
///
/// Holds two representations of the same facts: the registration payload, which
/// every response body is rendered from, and the `registration_information`
/// routing projection derived from its `information`, which discovery
/// preselects against. Every mutation below rewrites the projection in the same
/// transaction as the payload, so no reader observes one without the other.
#[derive(Debug)]
pub struct PostgresRegistrationStore {
    /// Tenant-scoped PostgreSQL connection pool.
    pool: PgPool,
    /// The tenant's attribute-name dictionary, which the routing projection
    /// interns registered property and relationship names through.
    attribute_names: Arc<dyn NameLookup<AttributeName>>,
}

impl PostgresRegistrationStore {
    /// Create a registration store backed by the supplied tenant pool and the
    /// tenant's shared attribute-name dictionary.
    pub fn new(pool: &PgPool, attribute_names: Arc<dyn NameLookup<AttributeName>>) -> PostgresRegistrationStore {
        PostgresRegistrationStore {
            pool: pool.clone(), // clone: `PgPool` is an atomically reference-counted pool handle.
            attribute_names,
        }
    }
}

/// Erase an adapter-specific diagnostic at the policy error boundary.
fn persistence_error(error: impl Display) -> RegistrationError {
    RegistrationError::persistence(error.to_string())
}

/// Preserve PostgreSQL unique violations as registration identifier conflicts.
fn create_error(error: sqlx::Error, identifier: &Uri) -> RegistrationError {
    match &error {
        sqlx::Error::Database(database_error) if database_error.code().as_deref() == Some("23505") => RegistrationError::already_exists(identifier),
        sqlx::Error::Configuration(_)
        | sqlx::Error::Database(_)
        | sqlx::Error::Io(_)
        | sqlx::Error::Tls(_)
        | sqlx::Error::Protocol(_)
        | sqlx::Error::RowNotFound
        | sqlx::Error::TypeNotFound { .. }
        | sqlx::Error::ColumnIndexOutOfBounds { .. }
        | sqlx::Error::ColumnNotFound(_)
        | sqlx::Error::ColumnDecode { .. }
        | sqlx::Error::Encode(_)
        | sqlx::Error::Decode(_)
        | sqlx::Error::AnyDriverError(_)
        | sqlx::Error::PoolTimedOut
        | sqlx::Error::PoolClosed
        | sqlx::Error::WorkerCrashed
        | sqlx::Error::Migrate(_)
        | sqlx::Error::InvalidSavePointStatement
        | sqlx::Error::BeginFailed
        | sqlx::Error::InvalidArgument(_)
        | _ => persistence_error(error),
    }
}

/// Map PostgreSQL affected-row counts to a backend-neutral mutation outcome.
const fn mutation_outcome(rows_affected: u64) -> RegistrationMutation {
    match rows_affected {
        0 => RegistrationMutation::Missing,
        1.. => RegistrationMutation::Applied,
    }
}

/// Convert PostgreSQL's signed count representation to the policy-owned count.
fn registration_count(total: i64) -> Result<u64, RegistrationError> {
    u64::try_from(total).map_err(persistence_error)
}

/// Add the full registration row projection to `select`, decoding each stored
/// geometry back to GeoJSON and each enum to text under the column name the
/// mapper reads it by.
fn apply_csr_projection(select: &mut SelectStatement) {
    select
        .column(Csr::Id)
        .column(Csr::Type)
        .column(Csr::RegistrationName)
        .column(Csr::Description)
        .column(Csr::Information)
        .column(Csr::DatasetId)
        .column(Csr::Tenant)
        .column(Csr::ObservationInterval)
        .column(Csr::ManagementInterval)
        .expr_as(geometry_as_geojson(SeaExpr::col(Csr::Location)), Csr::Location)
        .expr_as(geometry_as_geojson(SeaExpr::col(Csr::ObservationSpace)), Csr::ObservationSpace)
        .expr_as(geometry_as_geojson(SeaExpr::col(Csr::OperationSpace)), Csr::OperationSpace)
        .column(Csr::ExpiresAt)
        .column(Csr::Endpoint)
        .column(Csr::Accept)
        .expr_as(cast(SeaExpr::col(Csr::Mode), PostgresType::Text), Csr::Mode)
        .column(Csr::Operations)
        .column(Csr::Scope)
        .column(Csr::RefreshRate)
        .column(Csr::Management)
        .column(Csr::ContextSourceInfo)
        .column(Csr::JsonldContext)
        .column(Csr::ContextSourceAlias)
        .expr_as(cast(SeaExpr::col(Csr::Status), PostgresType::Text), Csr::Status)
        .column(Csr::TimesSent)
        .column(Csr::TimesFailed)
        .column(Csr::LastSuccess)
        .column(Csr::LastFailure)
        .column(Csr::CreatedAt)
        .column(Csr::ModifiedAt)
        .column(Csr::AdditionalProperties);
}

/// Build a registration selection statement with the canonical projection.
fn csr_base_select() -> SelectStatement {
    let mut select = Query::select();
    apply_csr_projection(&mut select);
    select.from(Csr::Table);
    select
}

/// The `CASE WHEN $marker IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($marker)) ELSE NULL END`
/// a geometry column is written through: the EWKB carries its SRID, and a NULL
/// EWKB projects to a NULL geometry rather than a call on a NULL argument.
fn geometry_write(marker: usize) -> sea_query::SimpleExpr {
    CaseStatement::new()
        .case(
            SeaExpr::expr(external_placeholder(marker)).is_not_null(),
            validated_geom_from_ewkb(external_placeholder(marker)),
        )
        .finally(Keyword::Null)
        .into()
}

/// The registration insert, its values bound by the caller's `.bind` chain in
/// the marker order this emits.
fn csr_create_sql() -> Result<String, RegistrationError> {
    let mut insert = Query::insert();
    insert.into_table(Csr::Table).columns([
        Csr::Id,
        Csr::Type,
        Csr::RegistrationName,
        Csr::Description,
        Csr::Information,
        Csr::DatasetId,
        Csr::Tenant,
        Csr::ObservationInterval,
        Csr::ManagementInterval,
        Csr::Location,
        Csr::ObservationSpace,
        Csr::OperationSpace,
        Csr::ExpiresAt,
        Csr::Endpoint,
        Csr::Accept,
        Csr::Mode,
        Csr::Operations,
        Csr::Scope,
        Csr::RefreshRate,
        Csr::Management,
        Csr::ContextSourceInfo,
        Csr::JsonldContext,
        Csr::ContextSourceAlias,
        Csr::AdditionalProperties,
        Csr::CreatedAt,
        Csr::ModifiedAt,
    ]);
    insert
        .values([
            external_placeholder(1),
            text_literal("ContextSourceRegistration"),
            external_placeholder(2),
            external_placeholder(3),
            external_placeholder(4),
            external_placeholder(5),
            external_placeholder(6),
            external_placeholder(7),
            external_placeholder(8),
            geometry_write(9),
            geometry_write(10),
            geometry_write(11),
            external_placeholder(12),
            external_placeholder(13),
            external_placeholder(14),
            cast(external_placeholder(15), PostgresType::CsrMode),
            external_placeholder(16),
            external_placeholder(17),
            external_placeholder(18),
            external_placeholder(19),
            external_placeholder(20),
            external_placeholder(21),
            external_placeholder(22),
            external_placeholder(23),
            now(),
            now(),
        ])
        .map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// The registration replacement, bound by the caller's `.bind` chain.
fn csr_update_sql() -> String {
    let mut update = Query::update();
    update
        .table(Csr::Table)
        .values([
            (Csr::RegistrationName, external_placeholder(2)),
            (Csr::Description, external_placeholder(3)),
            (Csr::Information, external_placeholder(4)),
            (Csr::DatasetId, external_placeholder(5)),
            (Csr::Tenant, external_placeholder(6)),
            (Csr::ObservationInterval, external_placeholder(7)),
            (Csr::ManagementInterval, external_placeholder(8)),
            (Csr::Location, geometry_write(9)),
            (Csr::ObservationSpace, geometry_write(10)),
            (Csr::OperationSpace, geometry_write(11)),
            (Csr::ExpiresAt, external_placeholder(12)),
            (Csr::Endpoint, external_placeholder(13)),
            (Csr::Accept, external_placeholder(14)),
            (Csr::Mode, cast(external_placeholder(15), PostgresType::CsrMode)),
            (Csr::Operations, external_placeholder(16)),
            (Csr::Scope, external_placeholder(17)),
            (Csr::RefreshRate, external_placeholder(18)),
            (Csr::Management, external_placeholder(19)),
            (Csr::ContextSourceInfo, external_placeholder(20)),
            (Csr::JsonldContext, external_placeholder(21)),
            (Csr::ContextSourceAlias, external_placeholder(22)),
            (Csr::AdditionalProperties, external_placeholder(23)),
            (Csr::ModifiedAt, now()),
        ])
        .and_where(SeaExpr::col(Csr::Id).eq(external_placeholder(1)));
    update.to_string(PostgresQueryBuilder)
}

/// The predicate matching registrations that have not expired: those with no
/// expiry, or whose expiry is still in the future.
fn not_expired() -> Cond {
    Cond::any()
        .add(SeaExpr::col(Csr::ExpiresAt).is_null())
        .add(SeaExpr::expr(SeaExpr::col(Csr::ExpiresAt)).gt(now()))
}

/// Encode supported operations using their PostgreSQL enum labels.
fn operation_names(operations: &[CsrOperation]) -> Vec<String> {
    operations.iter().map(ToString::to_string).collect()
}

/// Encode the `datasetId` list in its clause 4.5.5 wire form.
fn dataset_selectors(dataset_id: Option<&[DatasetSelector]>) -> Option<Vec<String>> {
    dataset_id.map(|values| values.iter().map(ToString::to_string).collect())
}

/// The health update one forwarding outcome applies (Table 5.2.9-2): the sent
/// counter always grows; the failed counter, the matching timestamp and the
/// status follow the outcome.
fn health_update_sql(outcome: ForwardingOutcome) -> String {
    let mut update = Query::update();
    update
        .table(Csr::Table)
        .value(Csr::TimesSent, SeaExpr::col(Csr::TimesSent).add(1))
        .and_where(SeaExpr::col(Csr::Id).eq(external_placeholder(1)));
    match outcome {
        ForwardingOutcome::Succeeded => {
            update
                .value(Csr::LastSuccess, external_placeholder(2))
                .value(Csr::Status, cast(text_literal(<&str>::from(CsrStatus::Ok)), PostgresType::CsrStatus));
        }
        ForwardingOutcome::Failed => {
            update
                .value(Csr::TimesFailed, SeaExpr::col(Csr::TimesFailed).add(1))
                .value(Csr::LastFailure, external_placeholder(2))
                .value(Csr::Status, cast(text_literal(<&str>::from(CsrStatus::Failed)), PostgresType::CsrStatus));
        }
    }
    update.to_string(PostgresQueryBuilder)
}

/// The predicate matching proxied registrations: mode `exclusive` or `redirect`.
fn proxied_mode() -> sea_query::SimpleExpr {
    cast(SeaExpr::col(Csr::Mode), PostgresType::Text).is_in([<&str>::from(ForwardingMode::Exclusive), <&str>::from(ForwardingMode::Redirect)])
}

#[async_trait]
impl RegistrationStore for PostgresRegistrationStore {
    async fn delete_expired(&self, cutoff: DateTime<Utc>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;
        // The rows to return are read before either deletion, so the projection
        // sees the geometries and enums the payload carried; the two deletions
        // then remove exactly the same set, agreeing without depending on the
        // foreign key's referential action. All three run in one transaction, so
        // no reader observes a partial removal.
        let mut expired_select = csr_base_select();
        expired_select
            .and_where(SeaExpr::col(Csr::ExpiresAt).is_not_null())
            .and_where(SeaExpr::col(Csr::ExpiresAt).lte(cutoff))
            .order_by(Csr::Id, Order::Asc);
        let (select_sql, select_values) = expired_select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(select_sql.as_str()), select_values)
            .fetch_all(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        let expired = rows
            .iter()
            .map(|row| row_to_csr(row).map_err(persistence_error))
            .collect::<Result<Vec<_>, _>>()?;

        let mut expired_ids = Query::select();
        expired_ids
            .column(Csr::Id)
            .from(Csr::Table)
            .and_where(SeaExpr::col(Csr::ExpiresAt).is_not_null())
            .and_where(SeaExpr::col(Csr::ExpiresAt).lte(cutoff));
        let delete_information = Query::delete()
            .from_table(RegistrationInformation::Table)
            .and_where(SeaExpr::col(RegistrationInformation::RegistrationId).in_subquery(expired_ids))
            .to_owned();
        let (info_sql, info_values) = delete_information.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(info_sql.as_str()), info_values)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;

        let delete_registrations = Query::delete()
            .from_table(Csr::Table)
            .and_where(SeaExpr::col(Csr::ExpiresAt).is_not_null())
            .and_where(SeaExpr::col(Csr::ExpiresAt).lte(cutoff))
            .to_owned();
        let (delete_sql, delete_values) = delete_registrations.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(delete_sql.as_str()), delete_values)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;

        transaction.commit().await.map_err(persistence_error)?;
        Ok(expired)
    }

    async fn exists(&self, id: &Uri) -> Result<bool, RegistrationError> {
        let inner = Query::select()
            .expr(SeaExpr::val(1))
            .from(Csr::Table)
            .and_where(SeaExpr::col(Csr::Id).eq(id.as_str()))
            .to_owned();
        let statement = Query::select().expr(SeaExpr::exists(inner)).to_owned();
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let row: (bool,) = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_one(&self.pool)
            .await
            .map_err(persistence_error)?;
        Ok(row.0)
    }

    async fn create(&self, id: &Uri, params: &CreateCsrParams) -> Result<(), RegistrationError> {
        let information = serde_json::to_value(&params.information)?;
        let observation_interval = params.observation_interval.map(serde_json::to_value).transpose()?;
        let management_interval = params.management_interval.map(serde_json::to_value).transpose()?;
        let management = params.management.as_ref().map(serde_json::to_value).transpose()?;
        let context_source_info = params.context_source_info.as_ref().map(serde_json::to_value).transpose()?;
        let operations = operation_names(&params.operations);
        let dataset_id = dataset_selectors(params.dataset_id.as_deref());
        let scopes = params.scope.as_deref().map(|values| values.iter().map(ScopeBuf::as_str).collect::<Vec<_>>());
        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;
        let create_sql = csr_create_sql()?;
        sqlx::query(sqlx::AssertSqlSafe(create_sql.as_str()))
            .bind(id.as_str())
            .bind(params.registration_name.as_ref().map(NonEmptyText::as_str))
            .bind(params.description.as_ref().map(NonEmptyText::as_str))
            .bind(information)
            .bind(dataset_id.as_deref())
            .bind(params.tenant.as_ref().map(TenantName::as_str))
            .bind(observation_interval)
            .bind(management_interval)
            .bind(geometry_bytes(params.location.as_ref()).map_err(persistence_error)?)
            .bind(geometry_bytes(params.observation_space.as_ref()).map_err(persistence_error)?)
            .bind(geometry_bytes(params.operation_space.as_ref()).map_err(persistence_error)?)
            .bind(params.expires_at)
            .bind(params.endpoint.as_str())
            .bind(params.accept.as_ref().map(ToString::to_string))
            .bind(<&str>::from(params.mode))
            .bind(operations.as_slice())
            .bind(scopes.as_deref())
            .bind(params.refresh_rate.as_ref().map(ToString::to_string))
            .bind(management)
            .bind(context_source_info)
            .bind(params.jsonld_context.as_ref().map(url::Url::as_str))
            .bind(params.context_source_alias.as_ref().map(ContextSourcePseudonym::as_str))
            .bind(&params.additional_properties)
            .execute(&mut *transaction)
            .await
            .map_err(|error| create_error(error, id))?;
        replace_registration_information(
            &mut transaction,
            id,
            &params.information,
            Some(&params.operations),
            self.attribute_names.as_ref(),
        )
        .await
        .map_err(persistence_error)?;
        transaction.commit().await.map_err(persistence_error)?;
        Ok(())
    }

    async fn get(&self, id: &Uri) -> Result<Option<ContextSourceRegistration>, RegistrationError> {
        let mut select = csr_base_select();
        select.and_where(SeaExpr::col(Csr::Id).eq(id.as_str()));
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_optional(&self.pool)
            .await
            .map_err(persistence_error)?;
        match row {
            Some(row) => Ok(Some(row_to_csr(&row).map_err(persistence_error)?)),
            None => Ok(None),
        }
    }

    async fn list(&self, pagination: Pagination) -> Result<RegistrationPage, RegistrationError> {
        let count = Query::select().expr(count_all()).from(Csr::Table).to_owned();
        let (count_sql, count_values) = count.build_sqlx(PostgresQueryBuilder);
        let count_row: (i64,) = sqlx::query_as_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
            .fetch_one(&self.pool)
            .await
            .map_err(persistence_error)?;
        let mut select = csr_base_select();
        select
            .order_by(Csr::CreatedAt, Order::Asc)
            .limit(u64::from(pagination.limit))
            .offset(u64::from(pagination.offset));
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.pool)
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

    async fn query_compiled(&self, compiled: &CompiledCsrQuery) -> Result<RegistrationPage, RegistrationError> {
        let built = csr_sql::build_csr_query(compiled).map_err(persistence_error)?;
        let (count_sql, count_values) = built.count.build_sqlx(PostgresQueryBuilder);
        let (total,): (i64,) = sqlx::query_as_with(sqlx::AssertSqlSafe(count_sql.as_str()), count_values)
            .fetch_one(&self.pool)
            .await
            .map_err(persistence_error)?;
        let (sql, values) = built.select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.pool)
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

    async fn update(&self, id: &Uri, values: &ResolvedCsrUpdate) -> Result<RegistrationMutation, RegistrationError> {
        let information = serde_json::to_value(&values.information)?;
        let observation_interval = values.observation_interval.map(serde_json::to_value).transpose()?;
        let management_interval = values.management_interval.map(serde_json::to_value).transpose()?;
        let management = values.management.as_ref().map(serde_json::to_value).transpose()?;
        let context_source_info = values.context_source_info.as_ref().map(serde_json::to_value).transpose()?;
        let operations = operation_names(&values.operations);
        let dataset_id = dataset_selectors(values.dataset_id.as_deref());
        let scopes = values.scope.as_deref().map(|values| values.iter().map(ScopeBuf::as_str).collect::<Vec<_>>());
        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;
        let update_sql = csr_update_sql();
        let result = sqlx::query(sqlx::AssertSqlSafe(update_sql.as_str()))
            .bind(id.as_str())
            .bind(values.registration_name.as_ref().map(NonEmptyText::as_str))
            .bind(values.description.as_ref().map(NonEmptyText::as_str))
            .bind(information)
            .bind(dataset_id.as_deref())
            .bind(values.tenant.as_ref().map(TenantName::as_str))
            .bind(observation_interval)
            .bind(management_interval)
            .bind(geometry_bytes(values.location.as_ref()).map_err(persistence_error)?)
            .bind(geometry_bytes(values.observation_space.as_ref()).map_err(persistence_error)?)
            .bind(geometry_bytes(values.operation_space.as_ref()).map_err(persistence_error)?)
            .bind(values.expires_at)
            .bind(values.endpoint.as_str())
            .bind(values.accept.as_ref().map(ToString::to_string))
            .bind(<&str>::from(values.mode))
            .bind(operations.as_slice())
            .bind(scopes.as_deref())
            .bind(values.refresh_rate.as_ref().map(ToString::to_string))
            .bind(management)
            .bind(context_source_info)
            .bind(values.jsonld_context.as_ref().map(url::Url::as_str))
            .bind(values.context_source_alias.as_ref().map(ContextSourcePseudonym::as_str))
            .bind(&values.additional_properties)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        let outcome = mutation_outcome(result.rows_affected());
        match outcome {
            // Nothing was replaced, so nothing derived from it may be rewritten:
            // a missing registration has no projection rows to speak for.
            RegistrationMutation::Missing => transaction.rollback().await.map_err(persistence_error)?,
            RegistrationMutation::Applied => {
                replace_registration_information(
                    &mut transaction,
                    id,
                    &values.information,
                    Some(&values.operations),
                    self.attribute_names.as_ref(),
                )
                .await
                .map_err(persistence_error)?;
                transaction.commit().await.map_err(persistence_error)?;
            }
        }
        Ok(outcome)
    }

    async fn find_matching(&self, entity_type: Option<&IriBuf>, entity_id: Option<&Uri>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
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
            .fetch_all(&self.pool)
            .await
            .map_err(persistence_error)?;
        rows.iter().map(|row| row_to_csr(row).map_err(persistence_error)).collect()
    }

    async fn find_overlapping(&self, entity_id: Option<&Uri>, attributes: &[IriBuf]) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
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
            .fetch_all(&self.pool)
            .await
            .map_err(persistence_error)?;
        rows.iter().map(|row| row_to_csr(row).map_err(persistence_error)).collect()
    }

    async fn find_all_active(&self, after: Option<&Uri>, limit: u32) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        let mut select = csr_base_select();
        select.cond_where(not_expired());
        if let Some(after_id) = after {
            select.and_where(SeaExpr::col(Csr::Id).gt(after_id.as_str()));
        }
        select.order_by(Csr::Id, Order::Asc).limit(u64::from(limit));
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_all(&self.pool)
            .await
            .map_err(persistence_error)?;
        rows.iter().map(|row| row_to_csr(row).map_err(persistence_error)).collect()
    }

    async fn record_forwarding_outcome(&self, id: &Uri, outcome: ForwardingOutcome, at: DateTime<Utc>) -> Result<(), RegistrationError> {
        let sql = health_update_sql(outcome);
        sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(id.as_str())
            .bind(at)
            .execute(&self.pool)
            .await
            .map_err(persistence_error)?;
        Ok(())
    }

    async fn delete(&self, id: &Uri) -> Result<RegistrationMutation, RegistrationError> {
        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;
        // Removed before the payload so the deletion holds whatever referential
        // action the foreign key carries; a registration that lingers in the
        // projection keeps being preselected for distributed operations.
        let delete_information = Query::delete()
            .from_table(RegistrationInformation::Table)
            .and_where(SeaExpr::col(RegistrationInformation::RegistrationId).eq(id.as_str()))
            .to_owned();
        let (info_sql, info_values) = delete_information.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(info_sql.as_str()), info_values)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;

        let delete_registration = Query::delete()
            .from_table(Csr::Table)
            .and_where(SeaExpr::col(Csr::Id).eq(id.as_str()))
            .to_owned();
        let (delete_sql, delete_values) = delete_registration.build_sqlx(PostgresQueryBuilder);
        let result = sqlx::query_with(sqlx::AssertSqlSafe(delete_sql.as_str()), delete_values)
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        transaction.commit().await.map_err(persistence_error)?;
        Ok(mutation_outcome(result.rows_affected()))
    }
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::registration_store::{csr_create_sql, csr_update_sql, health_update_sql, mutation_outcome};
    use andromeda_registration::store::{ForwardingOutcome, RegistrationMutation};

    #[test]
    fn the_create_writes_its_type_geometries_and_mode_with_contiguous_markers() {
        let sql = csr_create_sql().expect("the fixed registration insert builds");
        assert!(sql.contains("'ContextSourceRegistration'"), "{sql}");
        assert!(sql.contains("$15::csr_mode"), "{sql}");
        for marker in [9, 10, 11] {
            assert!(
                sql.contains(&format!("ST_MakeValid(ST_GeomFromEWKB(${marker}))")),
                "geometry {marker} is validated from its EWKB: {sql}"
            );
        }
        assert!(sql.contains("NOW()"), "created_at and modified_at stamp the transaction clock: {sql}");
        for marker in 1..=23 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
    }

    #[test]
    fn the_update_pins_the_row_by_id_and_decodes_its_geometries() {
        let sql = csr_update_sql();
        assert!(sql.contains(r#""id" = ($1)"#), "{sql}");
        assert!(sql.contains("$15::csr_mode"), "{sql}");
        assert!(sql.contains(r#""modified_at" = NOW()"#), "{sql}");
        assert!(sql.contains("ST_MakeValid(ST_GeomFromEWKB($9))"), "{sql}");
    }

    #[test]
    fn a_success_stamps_last_success_and_an_ok_status_while_a_failure_counts_and_stamps_the_failure() {
        let success = health_update_sql(ForwardingOutcome::Succeeded);
        assert!(success.contains(r#""times_sent" = "times_sent" + 1"#), "{success}");
        assert!(success.contains(r#""last_success" = $2"#), "{success}");
        assert!(success.contains("'ok'::csr_status"), "{success}");
        assert!(!success.contains("times_failed"), "{success}");
        let failure = health_update_sql(ForwardingOutcome::Failed);
        assert!(failure.contains(r#""times_failed" = "times_failed" + 1"#), "{failure}");
        assert!(failure.contains(r#""last_failure" = $2"#), "{failure}");
        assert!(failure.contains("'failed'::csr_status"), "{failure}");
        assert!(failure.contains(r#""id" = ($1)"#), "{failure}");
    }

    /// Zero affected rows preserve the policy-level missing outcome.
    #[test]
    fn zero_affected_rows_map_to_missing() {
        assert_eq!(mutation_outcome(0), RegistrationMutation::Missing);
    }

    /// Any positive affected-row count preserves the policy-level applied outcome.
    #[test]
    fn positive_affected_rows_map_to_applied() {
        assert_eq!(mutation_outcome(1), RegistrationMutation::Applied);
        assert_eq!(mutation_outcome(2), RegistrationMutation::Applied);
    }
}
