//! `RegistrationStore` trait implementation for `CsourceRegistrationRepository`.

use crate::{
    core::error::AndromedaError,
    service::{
        csource_merge::ResolvedCsrUpdate,
        csource_registration::store::{CreateCsrParams, RegistrationStore},
    },
};
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, uri::Uri};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::csr::compiled::CompiledCsrQuery;
use andromeda_postgresql_context_query::csr::{
    coverage::{entity_id_coverage, entity_type_coverage},
    statement as csr_sql,
};
use andromeda_postgresql_context_store::csource_registration::{mapper::row_to_csr, repo::CsourceRegistrationRepository};
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use async_trait::async_trait;
use iri_rs::IriBuf;
use sea_query::{Alias, Expr as SeaExpr, Order, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_binder::SqlxBinder;

fn csr_column_projection() -> sea_query::SimpleExpr {
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

fn csr_base_select() -> SelectStatement {
    let mut select = Query::select();
    select.expr(csr_column_projection()).from(Alias::new("context_source_registrations"));
    select
}

#[async_trait]
impl RegistrationStore for CsourceRegistrationRepository {
    async fn exists(&self, id: &str) -> Result<bool, AndromedaError> {
        let row: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM context_source_registrations WHERE id = $1)")
            .bind(id)
            .fetch_one(self.pool())
            .await?;
        Ok(row.0)
    }

    async fn create(&self, id: &str, params: &CreateCsrParams<'_>) -> Result<(), AndromedaError> {
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
        .bind(id)
        .bind(params.registration_name)
        .bind(params.description)
        .bind(params.information.cloned())
        .bind(params.tenant)
        .bind(params.observation_interval.cloned())
        .bind(params.management_interval.cloned())
        .bind(geometry_bytes(params.location.as_ref())?)
        .bind(geometry_bytes(params.observation_space.as_ref())?)
        .bind(geometry_bytes(params.operation_space.as_ref())?)
        .bind(params.expires_at)
        .bind(params.endpoint)
        .bind(params.accept)
        .bind(<&str>::from(params.mode))
        .bind(params.operations.as_deref())
        .bind(params.scope.as_deref())
        .bind(params.refresh_rate.as_ref().map(ToString::to_string))
        .bind(params.management.cloned())
        .bind(params.context_source_info.cloned())
        .bind(params.jsonld_context)
        .bind(params.context_source_alias)
        .bind(&params.additional_properties)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<ContextSourceRegistration>, AndromedaError> {
        let mut select = csr_base_select();
        select.and_where(SeaExpr::col(Alias::new("id")).eq(id));
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(&sql, values).fetch_optional(self.pool()).await?;
        match row {
            Some(row) => Ok(Some(row_to_csr(&row).map_err(AndromedaError::from)?)),
            None => Ok(None),
        }
    }

    async fn list(&self, limit: Option<i64>, offset: Option<i64>) -> Result<(Vec<ContextSourceRegistration>, i64), AndromedaError> {
        let limit = u64::try_from(limit.unwrap_or(100)).unwrap_or(100);
        let offset = u64::try_from(offset.unwrap_or(0)).unwrap_or(0);
        let count_row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM context_source_registrations")
            .fetch_one(self.pool())
            .await?;
        let mut select = csr_base_select();
        select.order_by(Alias::new("created_at"), Order::Asc).limit(limit).offset(offset);
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(&sql, values).fetch_all(self.pool()).await?;
        let csrs = rows
            .iter()
            .map(|row| row_to_csr(row).map_err(AndromedaError::from))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((csrs, count_row.0))
    }

    async fn query_compiled(&self, compiled: &CompiledCsrQuery) -> Result<(Vec<ContextSourceRegistration>, i64), AndromedaError> {
        let built = csr_sql::build_csr_query(compiled)?;
        let total = if let Some(ref count_sql) = built.count_sql {
            let count_query = andromeda_postgresql_query::bind_values!(sqlx::query_as::<_, (i64,)>(count_sql), &built.bind_values);
            let (total,) = count_query.fetch_one(self.pool()).await?;
            total
        } else {
            0
        };
        let main_query = andromeda_postgresql_query::bind_values!(sqlx::query(&built.sql), &built.bind_values);
        let rows = main_query.fetch_all(self.pool()).await?;
        let csrs = rows
            .iter()
            .map(|row| row_to_csr(row).map_err(AndromedaError::from))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((csrs, total))
    }

    async fn update(&self, id: &str, values: &ResolvedCsrUpdate) -> Result<u64, AndromedaError> {
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
        .bind(id)
        .bind(values.registration_name.as_deref())
        .bind(values.description.as_deref())
        .bind(&values.information)
        .bind(values.tenant.as_ref().map(TenantName::as_str))
        .bind(&values.observation_interval)
        .bind(&values.management_interval)
        .bind(geometry_bytes(values.location.as_ref())?)
        .bind(geometry_bytes(values.observation_space.as_ref())?)
        .bind(geometry_bytes(values.operation_space.as_ref())?)
        .bind(values.expires_at)
        .bind(values.endpoint.as_str())
        .bind(values.accept.as_deref())
        .bind(<&str>::from(values.mode))
        .bind(values.operations.as_deref())
        .bind(values.scope.as_deref())
        .bind(values.refresh_rate.as_ref().map(ToString::to_string))
        .bind(&values.management)
        .bind(&values.context_source_info)
        .bind(values.jsonld_context.as_deref())
        .bind(values.context_source_alias.as_deref())
        .bind(&values.additional_properties)
        .execute(self.pool())
        .await?;
        Ok(result.rows_affected())
    }

    async fn find_matching(&self, entity_type: Option<&str>, entity_id: Option<&str>) -> Result<Vec<ContextSourceRegistration>, AndromedaError> {
        use sea_query::Cond;
        let expires_at_col = Alias::new("expires_at");
        let not_expired = Cond::any().add(SeaExpr::col(expires_at_col).is_null()).add(SeaExpr::cust("expires_at > NOW()"));
        let mut select = csr_base_select();
        select.cond_where(not_expired);
        if let Some(entity_type) = entity_type {
            let expanded = IriBuf::new(entity_type.to_string())
                .map_err(|error| AndromedaError::internal(format!("Registration match received a malformed entity type IRI '{entity_type}': {error}")))?;
            select.and_where(entity_type_coverage(&expanded));
        }
        if let Some(entity_id) = entity_id {
            let entity_uri: Uri = entity_id
                .parse()
                .map_err(|error| AndromedaError::internal(format!("Registration match received a malformed entity id '{entity_id}': {error}")))?;
            select.and_where(entity_id_coverage(&entity_uri));
        }
        select.order_by(Alias::new("created_at"), Order::Asc);
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(&sql, values).fetch_all(self.pool()).await?;
        rows.iter().map(|row| row_to_csr(row).map_err(AndromedaError::from)).collect()
    }

    async fn find_all_active(&self, after: Option<&str>, limit: u32) -> Result<Vec<ContextSourceRegistration>, AndromedaError> {
        use sea_query::Cond;
        let expires_at_col = Alias::new("expires_at");
        let not_expired = Cond::any().add(SeaExpr::col(expires_at_col).is_null()).add(SeaExpr::cust("expires_at > NOW()"));
        let mut select = csr_base_select();
        select.cond_where(not_expired);
        if let Some(after_id) = after {
            select.and_where(SeaExpr::col(Alias::new("id")).gt(after_id));
        }
        select.order_by(Alias::new("id"), Order::Asc).limit(u64::from(limit));
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(&sql, values).fetch_all(self.pool()).await?;
        rows.iter().map(|row| row_to_csr(row).map_err(AndromedaError::from)).collect()
    }

    async fn delete(&self, id: &str) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM context_source_registrations WHERE id = $1")
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(result.rows_affected())
    }
}
