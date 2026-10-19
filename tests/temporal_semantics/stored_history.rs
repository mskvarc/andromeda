//! The stored temporal rows one verification test reads back from the tenant.
//!
//! Most temporal guarantees are properties of storage — which column a
//! timestamp landed in, which of the two instance-identity columns is
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
    pub generated_instance_id: Option<Uuid>,
    /// Client-supplied instance identifier, kept byte for byte.
    pub verbatim_instance_id: Option<String>,
    /// Exact decimal spelling of the numeric value column.
    pub value_number: Option<String>,
    /// String value column.
    pub value_string: Option<String>,
    /// URI value column, which a Relationship target occupies.
    pub value_uri: Option<String>,
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
            attribute_name: row.try_get("attribute_name").expect("the attribute_name column decodes"),
            stream_key: row.try_get("stream_key").expect("the stream_key column decodes"),
            dataset_id: row.try_get("dataset_id").expect("the dataset_id column decodes"),
            attribute_kind: row.try_get("attribute_kind").expect("the attribute_kind column decodes"),
            value_kind: row.try_get("value_kind").expect("the value_kind column decodes"),
            recorded_at: row.try_get("recorded_at").expect("the recorded_at column decodes"),
            write_id: row.try_get("write_id").expect("the write_id column decodes"),
            write_ordinal: row.try_get("write_ordinal").expect("the write_ordinal column decodes"),
            observed_at: row.try_get("observed_at").expect("the observed_at column decodes"),
            created_at: row.try_get("created_at").expect("the created_at column decodes"),
            modified_at: row.try_get("modified_at").expect("the modified_at column decodes"),
            deleted_at: row.try_get("deleted_at").expect("the deleted_at column decodes"),
            generated_instance_id: row.try_get("generated_instance_id").expect("the generated_instance_id column decodes"),
            verbatim_instance_id: row.try_get("verbatim_instance_id").expect("the verbatim_instance_id column decodes"),
            value_number: row.try_get("value_number").expect("the value_number column decodes"),
            value_string: row.try_get("value_string").expect("the value_string column decodes"),
            value_uri: row.try_get("value_uri").expect("the value_uri column decodes"),
            populated_value_columns: row.try_get("populated_value_columns").expect("the populated_value_columns column decodes"),
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
    sqlx::query(CURRENT_ATTRIBUTE_ROWS_SQL)
        .bind(entity_id)
        .fetch_one(pool)
        .await
        .expect("current projection count")
        .try_get("rows")
        .expect("the rows column decodes")
}

/// Count the temporal catalogue rows of one Entity.
pub async fn temporal_catalog_rows(pool: &PgPool, entity_id: &str) -> (i64, i64) {
    let row = sqlx::query(TEMPORAL_CATALOG_ROWS_SQL)
        .bind(entity_id)
        .fetch_one(pool)
        .await
        .expect("temporal catalogue counts");
    (
        row.try_get("entities").expect("the entities column decodes"),
        row.try_get("streams").expect("the streams column decodes"),
    )
}

/// Count the temporal rows whose owning catalogue row no longer exists.
///
/// The hypertables carry no foreign key back to the catalogue, so this audit is
/// what stands in for one: every count is expected to be zero.
pub async fn orphaned_temporal_rows(pool: &PgPool) -> (i64, i64, i64) {
    let row = sqlx::query(ORPHANED_TEMPORAL_ROWS_SQL).fetch_one(pool).await.expect("temporal orphan audit");
    (
        row.try_get("orphaned_attribute_history_rows")
            .expect("the orphaned_attribute_history_rows column decodes"),
        row.try_get("orphaned_scope_history_rows")
            .expect("the orphaned_scope_history_rows column decodes"),
        row.try_get("orphaned_streams").expect("the orphaned_streams column decodes"),
    )
}

const STORED_HISTORY_SQL: &str = r"
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
       ah.generated_instance_id,
       ah.verbatim_instance_id,
       ah.value_number::text AS value_number,
       ah.value_string,
       ah.value_uri,
       num_nonnulls(ah.value_number, ah.value_string, ah.value_boolean, ah.value_geometry,
                    ah.value_object, ah.value_date_time, ah.value_uri)::int AS populated_value_columns
FROM attribute_history ah
JOIN temporal_attribute_streams s ON s.stream_key = ah.stream_key
JOIN temporal_entities te ON te.temporal_entity_key = s.temporal_entity_key
JOIN attribute_names an ON an.attribute_name_key = s.attribute_name_key
WHERE te.id = $1
ORDER BY ah.recorded_at, ah.write_id, ah.write_ordinal
";

const CURRENT_ATTRIBUTE_ROWS_SQL: &str = r"
SELECT count(*)::bigint AS rows
FROM current_attributes c
JOIN entities e ON e.entity_key = c.entity_key
WHERE e.id = $1
";

const TEMPORAL_CATALOG_ROWS_SQL: &str = r"
SELECT (SELECT count(*)::bigint FROM temporal_entities WHERE id = $1) AS entities,
       (SELECT count(*)::bigint FROM temporal_attribute_streams s
        JOIN temporal_entities te ON te.temporal_entity_key = s.temporal_entity_key
        WHERE te.id = $1) AS streams
";

const ORPHANED_TEMPORAL_ROWS_SQL: &str = r"
SELECT
    (SELECT count(*)::bigint FROM attribute_history ah
        LEFT JOIN temporal_attribute_streams s ON s.stream_key = ah.stream_key
        WHERE s.stream_key IS NULL) AS orphaned_attribute_history_rows,
    (SELECT count(*)::bigint FROM scope_history sh
        LEFT JOIN temporal_entities te ON te.temporal_entity_key = sh.temporal_entity_key
        WHERE te.temporal_entity_key IS NULL) AS orphaned_scope_history_rows,
    (SELECT count(*)::bigint FROM temporal_attribute_streams s
        LEFT JOIN temporal_entities te ON te.temporal_entity_key = s.temporal_entity_key
        WHERE te.temporal_entity_key IS NULL) AS orphaned_streams
";
