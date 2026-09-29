//! Direct reads of the current-attribute projection for one Entity.

use sqlx::{FromRow, PgPool, Row};

/// The clause 4.5.5 grain of one projection row, with its typed value rendered
/// as text so a `NUMERIC` column needs no arbitrary-precision decoder on the
/// test side and still compares exactly.
#[derive(Debug, FromRow, PartialEq, Eq)]
pub struct ProjectionRow {
    /// Expanded attribute IRI taken from the `attribute_names` dictionary.
    pub attribute_name: String,
    /// `datasetId` of the instance, absent for the default instance.
    pub dataset_id: Option<String>,
    /// Migration-owned attribute-kind code (1 Property … 8 `ListRelationship`).
    pub attribute_kind: i16,
    /// Migration-owned value-kind code (1 Number … 7 Uri).
    pub value_kind: i16,
    /// The populated typed value column rendered as text.
    pub value_text: Option<String>,
}

/// SQL projecting one Entity's grains in a stable order.
const ROWS_FOR_ENTITY: &str = "SELECT an.name AS attribute_name, \
     c.dataset_id, \
     c.attribute_kind, \
     c.value_kind, \
     COALESCE(c.value_number::text, c.value_string, c.value_long_text, \
              c.value_boolean::text, c.value_date_time::text, c.value_uri, \
              c.value_long_uri, c.value_object::text, ST_AsText(c.value_geometry)) AS value_text \
     FROM current_attribute_values AS c \
     JOIN entities AS e ON e.entity_key = c.entity_key \
     JOIN attribute_names AS an ON an.attribute_name_key = c.attribute_name_key \
     WHERE e.id = $1 \
     ORDER BY an.name, c.dataset_id NULLS FIRST";

/// Read every projection row owned by one Entity identifier.
pub async fn rows_for_entity(pool: &PgPool, entity_id: &str) -> Vec<ProjectionRow> {
    sqlx::query_as(ROWS_FOR_ENTITY)
        .bind(entity_id)
        .fetch_all(pool)
        .await
        .expect("projection rows must be readable")
}

/// Read the rows of one attribute of one Entity, matched by the local name at
/// the end of its expanded IRI.
pub async fn rows_for_attribute(pool: &PgPool, entity_id: &str, local_name: &str) -> Vec<ProjectionRow> {
    rows_for_entity(pool, entity_id)
        .await
        .into_iter()
        .filter(|row| row.attribute_name.ends_with(local_name))
        .collect()
}

/// Read the surrogate key of one Entity, absent when no current Entity exists.
pub async fn entity_key(pool: &PgPool, entity_id: &str) -> Option<i64> {
    sqlx::query("SELECT entity_key FROM entities WHERE id = $1")
        .bind(entity_id)
        .fetch_optional(pool)
        .await
        .expect("entity lookup must succeed")
        .map(|row| row.get("entity_key"))
}

/// Count the projection rows carrying one surrogate key, including rows whose
/// Entity has already been deleted.
pub async fn rows_for_key(pool: &PgPool, key: i64) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM current_attribute_values WHERE entity_key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("projection row count must be readable")
}

/// Whether an independent temporal Entity exists for one identifier.
pub async fn temporal_entity_exists(pool: &PgPool, entity_id: &str) -> bool {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM temporal_entities WHERE id = $1")
        .bind(entity_id)
        .fetch_one(pool)
        .await
        .expect("temporal entity count must be readable")
        > 0
}

/// Read which of the two String columns holds the default instance of one
/// attribute, matched by the local name at the end of its expanded IRI, as
/// `(value_string, value_long_text)`. Exactly one side is present for a String
/// value: the indexed column up to the B-tree bound, the overflow column past it.
pub async fn string_placement(pool: &PgPool, entity_id: &str, local_name: &str) -> (Option<String>, Option<String>) {
    sqlx::query_as(
        "SELECT c.value_string, c.value_long_text FROM current_attribute_values AS c \
         JOIN entities AS e ON e.entity_key = c.entity_key \
         JOIN attribute_names AS an ON an.attribute_name_key = c.attribute_name_key \
         WHERE e.id = $1 AND c.dataset_id IS NULL AND an.name LIKE '%' || $2",
    )
    .bind(entity_id)
    .bind(local_name)
    .fetch_one(pool)
    .await
    .expect("the default instance of the attribute must be projected")
}

/// Read which of the two URI columns holds the default instance of one
/// attribute, matched by the local name at the end of its expanded IRI, as
/// `(value_uri, value_long_uri)`. Exactly one side is present for a
/// Relationship object: the traversed column up to the identifier bound, the
/// overflow column past it.
pub async fn uri_placement(pool: &PgPool, entity_id: &str, local_name: &str) -> (Option<String>, Option<String>) {
    sqlx::query_as(
        "SELECT c.value_uri, c.value_long_uri FROM current_attribute_values AS c \
         JOIN entities AS e ON e.entity_key = c.entity_key \
         JOIN attribute_names AS an ON an.attribute_name_key = c.attribute_name_key \
         WHERE e.id = $1 AND c.dataset_id IS NULL AND an.name LIKE '%' || $2",
    )
    .bind(entity_id)
    .bind(local_name)
    .fetch_one(pool)
    .await
    .expect("the default instance of the attribute must be projected")
}

/// Resolve the dictionary identifier of an expanded attribute IRI ending in the
/// supplied local name for one Entity.
pub async fn attribute_name_key(pool: &PgPool, entity_id: &str, local_name: &str) -> i32 {
    sqlx::query_scalar(
        "SELECT c.attribute_name_key FROM current_attribute_values AS c \
         JOIN entities AS e ON e.entity_key = c.entity_key \
         JOIN attribute_names AS an ON an.attribute_name_key = c.attribute_name_key \
         WHERE e.id = $1 AND an.name LIKE '%' || $2 LIMIT 1",
    )
    .bind(entity_id)
    .bind(local_name)
    .fetch_one(pool)
    .await
    .expect("the attribute must be present in the dictionary")
}

/// Read the `unitCode` projected for one instance of one attribute, matched by
/// the local name at the end of its expanded IRI, absent when the projected
/// instance carries none.
pub async fn unit_code_of(pool: &PgPool, entity_id: &str, local_name: &str, dataset_id: Option<&str>) -> Option<String> {
    sqlx::query_scalar(
        "SELECT u.code FROM current_attribute_values AS c \
         JOIN entities AS e ON e.entity_key = c.entity_key \
         JOIN attribute_names AS an ON an.attribute_name_key = c.attribute_name_key \
         LEFT JOIN unit_codes AS u ON u.id = c.unit_code_id \
         WHERE e.id = $1 AND an.name LIKE '%' || $2 AND c.dataset_id IS NOT DISTINCT FROM $3",
    )
    .bind(entity_id)
    .bind(local_name)
    .bind(dataset_id)
    .fetch_one(pool)
    .await
    .expect("the instance of the attribute must be projected")
}
