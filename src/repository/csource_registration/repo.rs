//! SQL-only repository for context source registrations.

use crate::{
    core::error::AndromedaError,
    domain::{registration::ContextSourceRegistration, uri::NGSI_LD_URN_NID},
    query::{compiled::CompiledCsrQuery, sql::csr as csr_sql},
    repository::{
        csource_registration::{mapper::row_to_csr, params::CreateCsrParams},
        geometry::encode_optional,
    },
    service::csource_merge::ResolvedCsrUpdate,
    tenancy::types::TenantName,
};
use sea_query::{Alias, Expr as SeaExpr, Order, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_binder::SqlxBinder;
use sqlx::postgres::PgPool;
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// Projection for a full CSR row. Static, identifier-only expression — no user
/// values — applied to every read path so `ST_AsGeoJSON` geometry casts stay
/// in one place.
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

/// PostgreSQL-backed repository for NGSI-LD context source registrations.
#[derive(Debug)]
pub struct CsourceRegistrationRepository {
    pool: PgPool,
}

impl CsourceRegistrationRepository {
    /// Creates a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> CsourceRegistrationRepository {
        CsourceRegistrationRepository { pool: pool.clone() } // clone: Arc ref-count bump
    }

    /// Generates a new unique URN identifier for a context source registration.
    pub fn generate_id() -> Result<String, AndromedaError> {
        let urn = UrnBuilder::new(NGSI_LD_URN_NID, &format!("ContextSourceRegistration:{}", Uuid::new_v4())).build()?;
        Ok(urn.to_string())
    }

    /// Checks whether a context source registration with the given ID exists.
    pub async fn exists(&self, id: &str) -> Result<bool, AndromedaError> {
        let row: (bool,) = sqlx::query_as("SELECT EXISTS(SELECT 1 FROM context_source_registrations WHERE id = $1)")
            .bind(id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }

    /// Persists a new context source registration with the given ID and parameters.
    pub async fn create(&self, id: &str, params: &CreateCsrParams<'_>) -> Result<(), AndromedaError> {
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
        .bind(encode_optional(params.location.as_ref())?)
        .bind(encode_optional(params.observation_space.as_ref())?)
        .bind(encode_optional(params.operation_space.as_ref())?)
        .bind(params.expires_at)
        .bind(params.endpoint)
        .bind(params.accept)
        .bind(<&str>::from(params.mode))
        .bind(params.operations.as_deref())
        .bind(params.scope.as_deref())
        .bind(params.refresh_rate)
        .bind(params.management.cloned())
        .bind(params.context_source_info.cloned())
        .bind(params.jsonld_context)
        .bind(params.context_source_alias)
        .bind(&params.additional_properties)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Retrieves a context source registration by ID.
    pub async fn get(&self, id: &str) -> Result<Option<ContextSourceRegistration>, AndromedaError> {
        let mut select = csr_base_select();
        select.and_where(SeaExpr::col(Alias::new("id")).eq(id));
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let row = sqlx::query_with(&sql, values).fetch_optional(&self.pool).await?;

        match row {
            Some(row) => Ok(Some(row_to_csr(&row)?)),
            None => Ok(None),
        }
    }

    /// Lists context source registrations with pagination. Returns entries and total count.
    pub async fn list(&self, limit: Option<i64>, offset: Option<i64>) -> Result<(Vec<ContextSourceRegistration>, i64), AndromedaError> {
        let limit = u64::try_from(limit.unwrap_or(100)).unwrap_or(100);
        let offset = u64::try_from(offset.unwrap_or(0)).unwrap_or(0);

        let count_row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM context_source_registrations")
            .fetch_one(&self.pool)
            .await?;

        let mut select = csr_base_select();
        select.order_by(Alias::new("created_at"), Order::Asc).limit(limit).offset(offset);
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(&sql, values).fetch_all(&self.pool).await?;

        let csrs = rows.iter().map(row_to_csr).collect::<Result<Vec<_>, _>>()?;

        Ok((csrs, count_row.0))
    }

    /// Executes a pre-compiled CSR query and returns matching registrations with total count.
    pub async fn query_compiled(&self, compiled: &CompiledCsrQuery) -> Result<(Vec<ContextSourceRegistration>, i64), AndromedaError> {
        let built = csr_sql::build_csr_query(compiled)?;

        let total = if let Some(ref count_sql) = built.count_sql {
            let count_query = crate::bind_values!(sqlx::query_as::<_, (i64,)>(count_sql), &built.bind_values);
            let (total,) = count_query.fetch_one(&self.pool).await?;
            total
        } else {
            0
        };

        let main_query = crate::bind_values!(sqlx::query(&built.sql), &built.bind_values);
        let rows = main_query.fetch_all(&self.pool).await?;

        let csrs = rows.iter().map(row_to_csr).collect::<Result<Vec<_>, _>>()?;

        Ok((csrs, total))
    }

    /// Unconditional UPDATE of an existing CSR with a fully-resolved set of
    /// column values. The service layer performs the three-way merge of the
    /// existing row + partial fragment before calling this method.
    pub async fn update(&self, id: &str, values: &ResolvedCsrUpdate) -> Result<u64, AndromedaError> {
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
        .bind(encode_optional(values.location.as_ref())?)
        .bind(encode_optional(values.observation_space.as_ref())?)
        .bind(encode_optional(values.operation_space.as_ref())?)
        .bind(values.expires_at)
        .bind(values.endpoint.as_str())
        .bind(values.accept.as_deref())
        .bind(<&str>::from(values.mode))
        .bind(values.operations.as_deref())
        .bind(values.scope.as_deref())
        .bind(values.refresh_rate.as_deref())
        .bind(&values.management)
        .bind(&values.context_source_info)
        .bind(values.jsonld_context.as_deref())
        .bind(values.context_source_alias.as_deref())
        .bind(&values.additional_properties)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Find CSRs matching an entity query.
    pub async fn find_matching(&self, entity_type: Option<&str>, entity_id: Option<&str>) -> Result<Vec<ContextSourceRegistration>, AndromedaError> {
        use sea_query::Cond;

        let expires_at_col = Alias::new("expires_at");
        let not_expired = Cond::any().add(SeaExpr::col(expires_at_col).is_null()).add(SeaExpr::cust("expires_at > NOW()"));

        let type_frag = "EXISTS (SELECT 1 FROM jsonb_array_elements(information) AS info \
             WHERE NOT jsonb_exists(info, 'entities') \
                OR info->'entities' = '[]'::jsonb \
                OR EXISTS (SELECT 1 FROM jsonb_array_elements(info->'entities') AS ent \
                   WHERE ent->>'type' = $1))";
        let id_frag = "EXISTS (SELECT 1 FROM jsonb_array_elements(information) AS info \
             WHERE NOT jsonb_exists(info, 'entities') \
                OR info->'entities' = '[]'::jsonb \
                OR EXISTS (SELECT 1 FROM jsonb_array_elements(info->'entities') AS ent \
                   WHERE (NOT jsonb_exists(ent, 'id') OR ent->>'id' = $1)))";

        let mut select = csr_base_select();
        select.cond_where(not_expired);

        match (entity_type, entity_id) {
            (Some(t), Some(id)) => {
                select.and_where(SeaExpr::cust_with_values(type_frag, [t.to_string()]));
                select.and_where(SeaExpr::cust_with_values(id_frag, [id.to_string()]));
            }
            (Some(t), None) => {
                select.and_where(SeaExpr::cust_with_values(type_frag, [t.to_string()]));
            }
            (None, Some(id)) => {
                select.and_where(SeaExpr::cust_with_values(id_frag, [id.to_string()]));
            }
            (None, None) => {}
        }

        select.order_by(Alias::new("created_at"), Order::Asc);
        let (sql, values) = select.build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(&sql, values).fetch_all(&self.pool).await?;
        rows.iter().map(row_to_csr).collect()
    }

    /// Return non-expired CSRs using keyset pagination on `id`.
    ///
    /// Callers pass the last seen id as `after` for subsequent pages; the
    /// first page passes `None`. The keyset ordering matches
    /// `jsonld_context::list` for consistency across discovery endpoints.
    pub async fn find_all_active(&self, after: Option<&str>, limit: u32) -> Result<Vec<ContextSourceRegistration>, AndromedaError> {
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
        let rows = sqlx::query_with(&sql, values).fetch_all(&self.pool).await?;
        rows.iter().map(row_to_csr).collect()
    }

    /// Deletes a context source registration by ID. Returns rows affected.
    pub async fn delete(&self, id: &str) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM context_source_registrations WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }
}
