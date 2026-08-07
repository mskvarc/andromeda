//! The aggregations behind entity type and attribute discovery (clause 5.7.3).
//!
//! Each statement aggregates across the whole `entities` table, so all of them
//! are keyset-paginated on their aggregate key. Query-plan assumption: the
//! server materialises the full aggregation before applying the keyset filter
//! and `LIMIT`, so the pagination caps transport volume rather than server-side
//! work. It exists to bound the response, not to make the aggregate cheaper.

use crate::{
    core::error::AndromedaError,
    repository::entity::{
        mapper::{stored_attribute_name, stored_type_name},
        repo::EntityRepository,
    },
    service::entity::store::AttributeSummary,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};

/// The aggregation shared by the paginated summary and the single-attribute
/// lookup, parameterised only by the predicate that selects attribute keys.
///
/// A macro rather than a function because the two differ by a `WHERE` clause
/// inside a `LATERAL` subquery and by their trailing clauses; copying the
/// aggregate into both would let a fix reach one and miss the other.
macro_rules! attribute_summary_sql {
    ($key_predicate:literal, $tail:literal) => {
        concat!(
            r#"
            SELECT attr_key,
                   COUNT(DISTINCT e.id) AS entity_count,
                   array_agg(DISTINCT t) AS entity_types,
                   array_agg(DISTINCT at) FILTER (WHERE at IS NOT NULL) AS attribute_types
            FROM entities e,
                 LATERAL (
                     SELECT key AS attr_key, value AS attr_value
                     FROM jsonb_each(
                         CASE jsonb_typeof(e.payload->0) WHEN 'object' THEN e.payload->0 ELSE e.payload END
                     )
                     WHERE "#,
            $key_predicate,
            r#"
                 ) attrs,
                 unnest(e.types) AS t
                 LEFT JOIN LATERAL jsonb_array_elements_text(attrs.attr_value->0->'@type') AS at ON true
            GROUP BY attr_key
            "#,
            $tail
        )
    };
}

/// Every attribute, keyset-paginated on the attribute IRI.
const ATTRIBUTE_SUMMARY_SQL: &str = attribute_summary_sql!(
    "key NOT LIKE '@%'",
    "HAVING $1::text IS NULL OR attr_key > $1\n            ORDER BY attr_key\n            LIMIT $2"
);

/// One named attribute.
const ATTRIBUTE_INFO_SQL: &str = attribute_summary_sql!("key = $1", "");

impl EntityRepository {
    /// Return all distinct entity type IRIs present in the store.
    pub async fn types(&self) -> Result<Vec<EntityTypeName>, AndromedaError> {
        let raw: Vec<String> = sqlx::query_scalar("SELECT DISTINCT unnest(types) FROM entities").fetch_all(&self.pool).await?;
        raw.into_iter().map(stored_type_name).collect()
    }

    /// Retrieve entity payloads filtered by type with optional pagination.
    pub async fn get_entities_by_type(&self, entity_type: &EntityTypeName, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, AndromedaError> {
        Ok(
            sqlx::query_scalar("SELECT payload FROM entities WHERE $1 = ANY(types) ORDER BY id LIMIT $2 OFFSET $3")
                .bind(entity_type.as_str())
                .bind(limit)
                .bind(offset.unwrap_or(0))
                .fetch_all(&self.pool)
                .await?,
        )
    }

    /// Retrieve all entity payloads with optional pagination.
    pub async fn get_all_entities(&self, limit: Option<i64>, offset: Option<i64>) -> Result<Vec<Value>, AndromedaError> {
        Ok(sqlx::query_scalar("SELECT payload FROM entities ORDER BY id LIMIT $1 OFFSET $2")
            .bind(limit)
            .bind(offset.unwrap_or(0))
            .fetch_all(&self.pool)
            .await?)
    }

    /// Count entities that have the given type IRI.
    pub async fn count_by_type(&self, entity_type: &EntityTypeName) -> Result<i64, AndromedaError> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM entities WHERE $1 = ANY(types)")
            .bind(entity_type.as_str())
            .fetch_one(&self.pool)
            .await?)
    }

    /// Per-type attribute summaries, keyset-paginated on the aggregated type IRI.
    pub async fn get_entity_type_summaries(
        &self,
        after: Option<&EntityTypeName>,
        limit: u32,
    ) -> Result<Vec<(EntityTypeName, Vec<AttributeName>)>, AndromedaError> {
        let rows = sqlx::query(
            r#"
            SELECT t AS type_iri,
                   COALESCE(array_agg(DISTINCT attr_key) FILTER (WHERE attr_key IS NOT NULL), '{}') AS attr_keys
            FROM entities,
                 unnest(types) AS t,
                 LATERAL jsonb_object_keys(
                     CASE jsonb_typeof(payload->0) WHEN 'object' THEN payload->0 ELSE payload END
                 ) AS attr_key
            WHERE attr_key NOT LIKE '@%'
            GROUP BY t
            HAVING $1::text IS NULL OR t > $1
            ORDER BY t
            LIMIT $2
            "#,
        )
        .bind(after.map(EntityTypeName::as_str))
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let type_iri = stored_type_name(row.get("type_iri"))?;
                let attr_keys: Vec<String> = row.get("attr_keys");
                let attr_names = attr_keys.into_iter().map(stored_attribute_name).collect::<Result<_, _>>()?;
                Ok((type_iri, attr_names))
            })
            .collect()
    }

    /// Attribute metadata aggregated at the SQL level, keyset-paginated on the
    /// attribute IRI.
    pub async fn get_attribute_summary(&self, after: Option<&AttributeName>, limit: u32) -> Result<Vec<AttributeSummary>, AndromedaError> {
        let rows = sqlx::query(ATTRIBUTE_SUMMARY_SQL)
            .bind(after.map(AttributeName::as_str))
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await?;

        rows.iter().map(row_to_attribute_summary).collect()
    }

    /// Attribute metadata for a single attribute IRI.
    pub async fn get_attribute_info(&self, attr_iri: &AttributeName) -> Result<Option<AttributeSummary>, AndromedaError> {
        let row = sqlx::query(ATTRIBUTE_INFO_SQL).bind(attr_iri.as_str()).fetch_optional(&self.pool).await?;
        row.as_ref().map(row_to_attribute_summary).transpose()
    }
}

/// An attribute has no `@type` when no instance of it declares one, which the
/// aggregate reports as `NULL` rather than an empty array.
///
/// The `@type` IRIs the payload carried are mapped to the eight NGSI-LD
/// attribute family names (clause 5.2.28); any value that is not one of them is
/// dropped, which is what the discovery response asks for.
fn row_to_attribute_summary(row: &PgRow) -> Result<AttributeSummary, AndromedaError> {
    let attribute_types: Option<Vec<String>> = row.get("attribute_types");
    let entity_types: Vec<String> = row.get("entity_types");
    Ok(AttributeSummary {
        attr_iri: stored_attribute_name(row.get("attr_key"))?,
        entity_count: row.get("entity_count"),
        entity_types: entity_types.into_iter().map(stored_type_name).collect::<Result<_, _>>()?,
        attribute_types: attribute_types
            .unwrap_or_default()
            .iter()
            .filter_map(|iri| AttributeType::from_iri_suffix(iri))
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_paginated_summary_and_the_single_lookup_share_one_aggregate() {
        // The two statements differ only by which attribute keys they select and
        // by the pagination tail; a fix to the aggregate must reach both.
        let shared = "COUNT(DISTINCT e.id) AS entity_count";
        assert!(ATTRIBUTE_SUMMARY_SQL.contains(shared));
        assert!(ATTRIBUTE_INFO_SQL.contains(shared));
        assert!(ATTRIBUTE_SUMMARY_SQL.contains("key NOT LIKE '@%'"));
        assert!(ATTRIBUTE_INFO_SQL.contains("key = $1"));
        assert!(!ATTRIBUTE_INFO_SQL.contains("LIMIT"), "the single-attribute lookup groups to one row");
    }
}
