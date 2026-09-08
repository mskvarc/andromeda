//! Direct PostgreSQL implementation of the JSON-LD resolver's technical cache port.

use crate::jsonld_context::{repo::JsonLdContextRepository, statement::payload_by_identifier_query};
use andromeda_jsonld::context_cache::{ContextCache, ContextCacheError};
use andromeda_ngsi_ld_lexicon::uri::NGSI_LD_URN_NID;
use andromeda_postgresql_schema::jsonld::JsonldContexts;
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use async_trait::async_trait;
use iri_rs::IriBuf;
use sea_query::{Expr, InsertStatement, OnConflict, PostgresQueryBuilder, Query, error::Error as SeaQueryError};
use sea_query_sqlx::SqlxBinder;
use serde_json::Value;
use sqlx::types::Json;
use urn_rs::UrnBuilder;
use uuid::Uuid;

/// The kind label a remotely-fetched Context is stored under.
const CACHED_KIND: &str = "Cached";

#[async_trait]
impl ContextCache for JsonLdContextRepository {
    async fn get(&self, url: &IriBuf) -> Result<Option<Value>, ContextCacheError> {
        let (sql, values) = payload_by_identifier_query(url.as_str()).build_sqlx(PostgresQueryBuilder);
        let row: Option<(Json<Value>,)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| ContextCacheError::Read(error.to_string()))?;
        Ok(row.map(|value| value.0.0))
    }

    async fn put(&self, url: &IriBuf, payload: &Value) -> Result<(), ContextCacheError> {
        let local_id = cached_context_identifier()?;
        let statement = cache_insert(&local_id, url.as_str(), payload).map_err(|error| ContextCacheError::Write(error.to_string()))?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(&self.pool)
            .await
            .map_err(|error| ContextCacheError::Write(error.to_string()))?;
        Ok(())
    }
}

/// Insert a remotely-fetched Context, leaving an already-cached URL untouched.
fn cache_insert(local_id: &str, url: &str, payload: &Value) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(JsonldContexts::Table)
        .columns([
            JsonldContexts::Id,
            JsonldContexts::Url,
            JsonldContexts::Kind,
            JsonldContexts::Payload,
            JsonldContexts::CreatedAt,
            JsonldContexts::LastUsage,
            JsonldContexts::NumberOfHits,
        ])
        .values([
            local_id.into(),
            url.into(),
            // A bound text parameter does not implicitly coerce to the enum
            // column the way an unknown-typed literal would, so the label is cast.
            cast(Expr::val(CACHED_KIND), PostgresType::JsonldContextKind),
            payload.clone().into(), // clone: the bound value owns the JSON for the round trip
            Expr::current_timestamp(),
            Expr::current_timestamp(),
            0_i64.into(),
        ])?
        .on_conflict(OnConflict::column(JsonldContexts::Url).do_nothing().to_owned());
    Ok(statement.clone())
}

/// Mint the local identifier used for a newly cached remote Context.
fn cached_context_identifier() -> Result<String, ContextCacheError> {
    UrnBuilder::new(NGSI_LD_URN_NID, &format!("Context:{}", Uuid::new_v4()))
        .build()
        .map(|identifier| identifier.to_string())
        .map_err(|error| ContextCacheError::Write(error.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::context_cache::cached_context_identifier;
    use andromeda_jsonld::context_cache::ContextCacheError;

    #[test]
    fn cached_context_identifier_uses_the_ngsi_ld_namespace() -> Result<(), ContextCacheError> {
        assert!(cached_context_identifier()?.starts_with("urn:ngsi-ld:Context:"));
        Ok(())
    }
}
