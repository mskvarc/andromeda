//! PostgreSQL implementation of the registration persistence port.

use crate::csource_registration::{information_projection::replace_registration_information, mapper::row_to_csr};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, scope::grammar::ScopeBuf, tenant::TenantName, uri::Uri};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, CsrOperation};
use andromeda_ngsi_ld_query::{csr::compiled::CompiledCsrQuery, pagination::Pagination};
use andromeda_postgresql_attribute_dictionary::attribute_name_lookup::AttributeNameLookup;
use andromeda_postgresql_context_query::csr::{
    coverage::{entity_id_coverage, entity_type_coverage},
    statement as csr_sql,
};
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_registration::{
    error::RegistrationError,
    resolved_update::ResolvedCsrUpdate,
    store::{CreateCsrParams, RegistrationMutation, RegistrationPage, RegistrationStore},
};
use async_trait::async_trait;
use iri_rs::IriBuf;
use sea_query::{Alias, Expr as SeaExpr, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_binder::SqlxBinder;
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
    attribute_names: Arc<dyn AttributeNameLookup>,
}

impl PostgresRegistrationStore {
    /// Create a registration store backed by the supplied tenant pool and the
    /// tenant's shared attribute-name dictionary.
    pub fn new(pool: &PgPool, attribute_names: Arc<dyn AttributeNameLookup>) -> PostgresRegistrationStore {
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
        | sqlx::Error::Migrate(_) => persistence_error(error),
        other => persistence_error(other),
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

/// Build the full registration row projection, including geometry decoding.
fn csr_column_projection() -> SimpleExpr {
    SeaExpr::cust(
        "id, type, registration_name, description, information, \
         tenant, observation_interval, management_interval, \
         ST_AsGeoJSON(location)::JSONB AS location, \
         ST_AsGeoJSON(observation_space)::JSONB AS observation_space, \
         ST_AsGeoJSON(operation_space)::JSONB AS operation_space, \
         expires_at, endpoint, accept, mode::text AS mode, \
         operations, scope, refresh_rate, \
         management, context_source_info, jsonld_context, context_source_alias, \
         status::text AS status, times_sent, times_failed, \
         last_success, last_failure, created_at, modified_at, \
         additional_properties",
    )
}

/// Build a registration selection statement with the canonical projection.
fn csr_base_select() -> SelectStatement {
    let mut select = Query::select();
    select.expr(csr_column_projection()).from(Alias::new("context_source_registrations"));
    select
}

/// Encode supported operations using their PostgreSQL enum labels.
fn operation_names(operations: Option<&[CsrOperation]>) -> Option<Vec<String>> {
    operations.map(|values| values.iter().map(ToString::to_string).collect())
}

#[async_trait]
impl RegistrationStore for PostgresRegistrationStore {
    async fn delete_expired(&self, cutoff: chrono::DateTime<chrono::Utc>) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;
        // The projection rows go first and are selected by the same predicate as
        // the deletion below, so the two statements agree on which registrations
        // expired without depending on the foreign key's referential action.
        sqlx::query(
            r#"
            DELETE FROM registration_information
            WHERE registration_id IN (
                SELECT id FROM context_source_registrations
                WHERE expires_at IS NOT NULL AND expires_at <= $1
            )
            "#,
        )
        .bind(cutoff)
        .execute(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        let rows = sqlx::query(
            r#"
            WITH expired AS (
                DELETE FROM context_source_registrations
                WHERE expires_at IS NOT NULL AND expires_at <= $1
                RETURNING *
            )
            SELECT id, type, registration_name, description, information,
                tenant, observation_interval, management_interval,
                ST_AsGeoJSON(location)::JSONB AS location,
                ST_AsGeoJSON(observation_space)::JSONB AS observation_space,
                ST_AsGeoJSON(operation_space)::JSONB AS operation_space,
                expires_at, endpoint, accept, mode::text AS mode,
                operations, scope, refresh_rate, management, context_source_info,
                jsonld_context, context_source_alias, status::text AS status,
                times_sent, times_failed, last_success, last_failure,
                created_at, modified_at, additional_properties
            FROM expired
            ORDER BY id
            "#,
        )
        .bind(cutoff)
        .fetch_all(&mut *transaction)
        .await
        .map_err(persistence_error)?;
        let expired = rows
            .iter()
            .map(|row| row_to_csr(row).map_err(persistence_error))
            .collect::<Result<Vec<_>, _>>()?;
        transaction.commit().await.map_err(persistence_error)?;
        Ok(expired)
    }

    async fn exists(&self, id: &Uri) -> Result<bool, RegistrationError> {
        let row: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM context_source_registrations WHERE id = $1)")
            .bind(id.as_str())
            .fetch_one(&self.pool)
            .await
            .map_err(persistence_error)?;
        Ok(row.0)
    }

    async fn create(&self, id: &Uri, params: &CreateCsrParams<'_>) -> Result<(), RegistrationError> {
        let information = params.information.as_ref().map(serde_json::to_value).transpose()?;
        let context_source_info = params.context_source_info.as_ref().map(serde_json::to_value).transpose()?;
        let operations = operation_names(params.operations.as_deref());
        let scopes = params.scope.as_deref().map(|values| values.iter().map(ScopeBuf::as_str).collect::<Vec<_>>());
        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;
        sqlx::query(
            r#"
            INSERT INTO context_source_registrations (
                id, type, registration_name, description, information,
                tenant, observation_interval, management_interval,
                location, observation_space, operation_space,
                expires_at, endpoint, accept, mode,
                operations, scope, refresh_rate,
                management, context_source_info, jsonld_context,
                context_source_alias, additional_properties,
                created_at, modified_at
            ) VALUES (
                $1, 'ContextSourceRegistration', $2, $3, $4,
                $5, $6, $7,
                CASE WHEN $8 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($8)) ELSE NULL END,
                CASE WHEN $9 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($9)) ELSE NULL END,
                CASE WHEN $10 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($10)) ELSE NULL END,
                $11, $12, $13, $14::csr_mode,
                $15, $16, $17,
                $18, $19, $20,
                $21, $22,
                NOW(), NOW()
            )
            "#,
        )
        .bind(id.as_str())
        .bind(params.registration_name)
        .bind(params.description)
        .bind(information)
        .bind(params.tenant.as_ref().map(TenantName::as_str))
        .bind(params.observation_interval.cloned())
        .bind(params.management_interval.cloned())
        .bind(geometry_bytes(params.location.as_ref()).map_err(persistence_error)?)
        .bind(geometry_bytes(params.observation_space.as_ref()).map_err(persistence_error)?)
        .bind(geometry_bytes(params.operation_space.as_ref()).map_err(persistence_error)?)
        .bind(params.expires_at)
        .bind(params.endpoint.as_ref().map(url::Url::as_str))
        .bind(params.accept.as_ref().map(ToString::to_string))
        .bind(<&str>::from(params.mode))
        .bind(operations.as_deref())
        .bind(scopes.as_deref())
        .bind(params.refresh_rate.as_ref().map(ToString::to_string))
        .bind(params.management.cloned())
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
            params.information.as_deref().unwrap_or_default(),
            params.operations.as_deref(),
            self.attribute_names.as_ref(),
        )
        .await
        .map_err(persistence_error)?;
        transaction.commit().await.map_err(persistence_error)?;
        Ok(())
    }

    async fn get(&self, id: &Uri) -> Result<Option<ContextSourceRegistration>, RegistrationError> {
        let mut select = csr_base_select();
        select.and_where(SeaExpr::col(Alias::new("id")).eq(id.as_str()));
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(&sql, values).fetch_optional(&self.pool).await.map_err(persistence_error)?;
        match row {
            Some(row) => Ok(Some(row_to_csr(&row).map_err(persistence_error)?)),
            None => Ok(None),
        }
    }

    async fn list(&self, pagination: Pagination) -> Result<RegistrationPage, RegistrationError> {
        let count_row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM context_source_registrations")
            .fetch_one(&self.pool)
            .await
            .map_err(persistence_error)?;
        let mut select = csr_base_select();
        select
            .order_by(Alias::new("created_at"), Order::Asc)
            .limit(u64::from(pagination.limit))
            .offset(u64::from(pagination.offset));
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(&sql, values).fetch_all(&self.pool).await.map_err(persistence_error)?;
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
        let total = if let Some(ref count_sql) = built.count_sql {
            let count_query = andromeda_postgresql_seaquery::bind_values!(sqlx::query_as::<_, (i64,)>(count_sql), &built.bind_values);
            let (total,) = count_query.fetch_one(&self.pool).await.map_err(persistence_error)?;
            total
        } else {
            0
        };
        let main_query = andromeda_postgresql_seaquery::bind_values!(sqlx::query(&built.sql), &built.bind_values);
        let rows = main_query.fetch_all(&self.pool).await.map_err(persistence_error)?;
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
        let context_source_info = values.context_source_info.as_ref().map(serde_json::to_value).transpose()?;
        let operations = operation_names(values.operations.as_deref());
        let scopes = values.scope.as_deref().map(|values| values.iter().map(ScopeBuf::as_str).collect::<Vec<_>>());
        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;
        let result = sqlx::query(
            r#"
            UPDATE context_source_registrations SET
                registration_name = $2,
                description = $3,
                information = $4,
                tenant = $5,
                observation_interval = $6,
                management_interval = $7,
                location = CASE WHEN $8 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($8)) ELSE NULL END,
                observation_space = CASE WHEN $9 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($9)) ELSE NULL END,
                operation_space = CASE WHEN $10 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($10)) ELSE NULL END,
                expires_at = $11,
                endpoint = $12,
                accept = $13,
                mode = $14::csr_mode,
                operations = $15,
                scope = $16,
                refresh_rate = $17,
                management = $18,
                context_source_info = $19,
                jsonld_context = $20,
                context_source_alias = $21,
                additional_properties = $22,
                modified_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(id.as_str())
        .bind(values.registration_name.as_deref())
        .bind(values.description.as_deref())
        .bind(information)
        .bind(values.tenant.as_ref().map(TenantName::as_str))
        .bind(&values.observation_interval)
        .bind(&values.management_interval)
        .bind(geometry_bytes(values.location.as_ref()).map_err(persistence_error)?)
        .bind(geometry_bytes(values.observation_space.as_ref()).map_err(persistence_error)?)
        .bind(geometry_bytes(values.operation_space.as_ref()).map_err(persistence_error)?)
        .bind(values.expires_at)
        .bind(values.endpoint.as_str())
        .bind(values.accept.as_ref().map(ToString::to_string))
        .bind(<&str>::from(values.mode))
        .bind(operations.as_deref())
        .bind(scopes.as_deref())
        .bind(values.refresh_rate.as_ref().map(ToString::to_string))
        .bind(&values.management)
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
                    values.operations.as_deref(),
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
        use sea_query::Cond;
        let expires_at_col = Alias::new("expires_at");
        let not_expired = Cond::any().add(SeaExpr::col(expires_at_col).is_null()).add(SeaExpr::cust("expires_at > NOW()"));
        let mut select = csr_base_select();
        select.cond_where(not_expired);
        if let Some(entity_type) = entity_type {
            select.and_where(entity_type_coverage(entity_type));
        }
        if let Some(entity_id) = entity_id {
            select.and_where(entity_id_coverage(entity_id));
        }
        select.order_by(Alias::new("created_at"), Order::Asc);
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(&sql, values).fetch_all(&self.pool).await.map_err(persistence_error)?;
        rows.iter().map(|row| row_to_csr(row).map_err(persistence_error)).collect()
    }

    async fn find_all_active(&self, after: Option<&Uri>, limit: u32) -> Result<Vec<ContextSourceRegistration>, RegistrationError> {
        use sea_query::Cond;
        let expires_at_col = Alias::new("expires_at");
        let not_expired = Cond::any().add(SeaExpr::col(expires_at_col).is_null()).add(SeaExpr::cust("expires_at > NOW()"));
        let mut select = csr_base_select();
        select.cond_where(not_expired);
        if let Some(after_id) = after {
            select.and_where(SeaExpr::col(Alias::new("id")).gt(after_id.as_str()));
        }
        select.order_by(Alias::new("id"), Order::Asc).limit(u64::from(limit));
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(&sql, values).fetch_all(&self.pool).await.map_err(persistence_error)?;
        rows.iter().map(|row| row_to_csr(row).map_err(persistence_error)).collect()
    }

    async fn delete(&self, id: &Uri) -> Result<RegistrationMutation, RegistrationError> {
        let mut transaction = self.pool.begin().await.map_err(persistence_error)?;
        // Removed before the payload so the deletion holds whatever referential
        // action the foreign key carries; a registration that lingers in the
        // projection keeps being preselected for distributed operations.
        sqlx::query("DELETE FROM registration_information WHERE registration_id = $1")
            .bind(id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        let result = sqlx::query("DELETE FROM context_source_registrations WHERE id = $1")
            .bind(id.as_str())
            .execute(&mut *transaction)
            .await
            .map_err(persistence_error)?;
        transaction.commit().await.map_err(persistence_error)?;
        Ok(mutation_outcome(result.rows_affected()))
    }
}

#[cfg(test)]
mod tests {
    use crate::csource_registration::registration_store::mutation_outcome;
    use andromeda_registration::store::RegistrationMutation;

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
