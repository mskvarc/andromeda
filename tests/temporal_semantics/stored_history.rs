//! The stored temporal rows one verification test reads back from the tenant.
//!
//! The matrix states most temporal guarantees as properties of storage — which
//! column a timestamp landed in, which of the two instance-identity columns is
//! populated, whether a tombstone carries a typed value — so the suite reads
//! the rows themselves rather than inferring them from a rendering that could
//! agree with the specification while the stored row does not.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// One `attribute_history` row, with the catalogue names its keys resolve to.
#[derive(Clone, Debug)]
pub struct StoredHistoryRow {
    /// Expanded attribute name the row's stream is keyed by.
    pub attribute_name: String,
    /// Stream the row belongs to.
    pub stream_key: i64,
    /// `datasetId` of the stream, absent for the default instance.
    pub dataset_id: Option<String>,
    /// Migration-owned attribute-family code carried on the row.
    pub attribute_kind: i16,
    /// Migration-owned value-shape code, `0` for a tombstone.
    pub value_kind: i16,
    /// Operational write clock and hypertable partition column.
    pub recorded_at: DateTime<Utc>,
    /// Identity of the atomic write the row belongs to.
    pub write_id: Uuid,
    /// Position of the row inside its write.
    pub write_ordinal: i32,
    /// `observedAt` axis, present only when the client declared one.
    pub observed_at: Option<DateTime<Utc>>,
    /// `createdAt` axis.
    pub created_at: Option<DateTime<Utc>>,
    /// `modifiedAt` axis.
    pub modified_at: Option<DateTime<Utc>>,
    /// `deletedAt` axis, present exactly on a tombstone.
    pub deleted_at: Option<DateTime<Utc>>,
    /// Broker-minted instance identifier, when the row carries one.
    pub generated_instance_uuid: Option<Uuid>,
    /// Client-supplied instance identifier, kept byte for byte.
    pub verbatim_instance_id: Option<String>,
    /// Exact decimal spelling of the numeric value column.
    pub value_number: Option<String>,
    /// String value column.
    pub value_string: Option<String>,
    /// URI value column, which a Relationship target occupies.
    pub value_uri: Option<String>,
    /// Extracted unit code identity, absent for a unitless instance.
    pub unit_code_id: Option<i16>,
    /// How many typed value columns the row populates.
    pub populated_value_columns: i32,
}

/// Read every history row recorded for one Entity, in write order.
pub async fn stored_history(pool: &PgPool, entity_id: &str) -> Vec<StoredHistoryRow> {
    sqlx::query(STORED_HISTORY_SQL)
        .bind(entity_id)
        .fetch_all(pool)
        .await
        .expect("stored attribute history")
        .into_iter()
        .map(|row| StoredHistoryRow {
            attribute_name: row.get("attribute_name"),
            stream_key: row.get("stream_key"),
            dataset_id: row.get("dataset_id"),
            attribute_kind: row.get("attribute_kind"),
            value_kind: row.get("value_kind"),
            recorded_at: row.get("recorded_at"),
            write_id: row.get("write_id"),
            write_ordinal: row.get("write_ordinal"),
            observed_at: row.get("observed_at"),
            created_at: row.get("created_at"),
            modified_at: row.get("modified_at"),
            deleted_at: row.get("deleted_at"),
            generated_instance_uuid: row.get("generated_instance_uuid"),
            verbatim_instance_id: row.get("verbatim_instance_id"),
            value_number: row.get("value_number"),
            value_string: row.get("value_string"),
            value_uri: row.get("value_uri"),
            unit_code_id: row.get("unit_code_id"),
            populated_value_columns: row.get("populated_value_columns"),
        })
        .collect()
}

/// Read the rows of one attribute of one Entity, in write order.
pub async fn stored_attribute_history(pool: &PgPool, entity_id: &str, attribute_name: &str) -> Vec<StoredHistoryRow> {
    stored_history(pool, entity_id)
        .await
        .into_iter()
        .filter(|row| row.attribute_name == attribute_name)
        .collect()
}

/// Count the current projection rows of one Entity.
pub async fn current_projection_rows(pool: &PgPool, entity_id: &str) -> i64 {
    sqlx::query(CURRENT_PROJECTION_ROWS_SQL)
        .bind(entity_id)
        .fetch_one(pool)
        .await
        .expect("current projection count")
        .get("rows")
}

/// Count the temporal catalogue rows of one Entity.
pub async fn temporal_catalog_rows(pool: &PgPool, entity_id: &str) -> (i64, i64) {
    let row = sqlx::query(TEMPORAL_CATALOG_ROWS_SQL)
        .bind(entity_id)
        .fetch_one(pool)
        .await
        .expect("temporal catalogue counts");
    (row.get("entities"), row.get("streams"))
}

/// Count the temporal rows whose owning catalogue row no longer exists.
///
/// The hypertables carry no foreign key back to the catalogue, so this audit is
/// what stands in for one: every count is expected to be zero.
pub async fn orphaned_temporal_rows(pool: &PgPool) -> (i64, i64, i64) {
    let row = sqlx::query(ORPHANED_TEMPORAL_ROWS_SQL).fetch_one(pool).await.expect("temporal orphan audit");
    (
        row.get("orphaned_attribute_history_rows"),
        row.get("orphaned_scope_history_rows"),
        row.get("orphaned_streams"),
    )
}

const STORED_HISTORY_SQL: &str = r#"
SELECT an.name AS attribute_name,
       s.stream_key AS stream_key,
       s.dataset_id,
       ah.attribute_kind,
       ah.value_kind,
       ah.recorded_at,
       ah.write_id,
       ah.write_ordinal,
       ah.observed_at,
       ah.created_at,
       ah.modified_at,
       ah.deleted_at,
       ah.generated_instance_uuid,
       ah.verbatim_instance_id,
       ah.value_number::text AS value_number,
       ah.value_string,
       ah.value_uri,
       ah.unit_code_id,
       num_nonnulls(ah.value_number, ah.value_string, ah.value_bool, ah.value_geo,
                    ah.value_object, ah.value_datetime, ah.value_uri)::int AS populated_value_columns
FROM attribute_history ah
JOIN temporal_attribute_streams s ON s.stream_key = ah.stream_key
JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key
JOIN attribute_names an ON an.id = s.attribute_name_id
WHERE te.id = $1
ORDER BY ah.recorded_at, ah.write_id, ah.write_ordinal
"#;

const CURRENT_PROJECTION_ROWS_SQL: &str = r#"
SELECT count(*)::bigint AS rows
FROM current_attribute_values c
JOIN entities e ON e.entity_key = c.entity_key
WHERE e.id = $1
"#;

const TEMPORAL_CATALOG_ROWS_SQL: &str = r#"
SELECT (SELECT count(*)::bigint FROM temporal_entities WHERE id = $1) AS entities,
       (SELECT count(*)::bigint FROM temporal_attribute_streams s
        JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key
        WHERE te.id = $1) AS streams
"#;

const ORPHANED_TEMPORAL_ROWS_SQL: &str = r#"
SELECT
    (SELECT count(*)::bigint FROM attribute_history ah
        LEFT JOIN temporal_attribute_streams s ON s.stream_key = ah.stream_key
        WHERE s.stream_key IS NULL) AS orphaned_attribute_history_rows,
    (SELECT count(*)::bigint FROM scope_history sh
        LEFT JOIN temporal_entities te ON te.entity_key = sh.temporal_entity_key
        WHERE te.entity_key IS NULL) AS orphaned_scope_history_rows,
    (SELECT count(*)::bigint FROM temporal_attribute_streams s
        LEFT JOIN temporal_entities te ON te.entity_key = s.temporal_entity_key
        WHERE te.entity_key IS NULL) AS orphaned_streams
"#;
