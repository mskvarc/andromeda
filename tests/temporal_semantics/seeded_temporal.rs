//! Seeding temporal rows directly, for populations no API request can produce.
//!
//! Two matrix rows need storage the broker never writes today: instances whose
//! broker-minted identifiers are migrated version-4 UUIDs, and history old
//! enough for an operator retention horizon to age out. Both are properties of
//! migrated tenants rather than of new writes, so the fixtures are inserted
//! through the tenant connection and then read back through the ordinary paths.

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// One directly seeded `attribute_history` row.
pub struct SeededHistoryRow {
    /// Stream the row belongs to.
    pub stream_key: i64,
    /// Operational write clock, which decides the chunk the row lands in.
    pub recorded_at: DateTime<Utc>,
    /// Identity of the write the row is attributed to.
    pub write_id: Uuid,
    /// Broker-minted instance identifier of the row.
    pub generated_instance_uuid: Uuid,
    /// Declared observation instant.
    pub observed_at: DateTime<Utc>,
    /// Numeric value the row carries.
    pub value_number: i32,
}

/// Insert a temporal Entity and return its narrow key.
pub async fn seed_temporal_entity(pool: &PgPool, entity_id: &str, entity_type: &str) -> i64 {
    sqlx::query(SEED_TEMPORAL_ENTITY_SQL)
        .bind(entity_id)
        .bind(entity_type)
        .fetch_one(pool)
        .await
        .expect("seeded temporal entity")
        .get("entity_key")
}

/// Resolve, creating if needed, the dictionary identity of one attribute name.
pub async fn attribute_name_id(pool: &PgPool, attribute_name: &str) -> i32 {
    sqlx::query(ATTRIBUTE_NAME_ID_SQL)
        .bind(attribute_name)
        .fetch_one(pool)
        .await
        .expect("attribute dictionary identity")
        .get("id")
}

/// Insert one attribute stream and return the key the database generated for it.
pub async fn seed_stream(pool: &PgPool, temporal_entity_key: i64, attribute_name_id: i32) -> i64 {
    sqlx::query(SEED_STREAM_SQL)
        .bind(temporal_entity_key)
        .bind(attribute_name_id)
        .fetch_one(pool)
        .await
        .expect("seeded attribute stream")
        .get("stream_key")
}

/// Insert one history row and the idempotency record that protects it.
pub async fn seed_history_row(pool: &PgPool, row: &SeededHistoryRow) {
    sqlx::query(SEED_WRITE_RECORD_SQL)
        .bind(row.write_id)
        .bind(row.recorded_at)
        .execute(pool)
        .await
        .expect("seeded idempotency record");
    sqlx::query(SEED_HISTORY_ROW_SQL)
        .bind(row.recorded_at)
        .bind(row.write_id)
        .bind(row.stream_key)
        .bind(row.generated_instance_uuid)
        .bind(row.observed_at)
        .bind(row.value_number)
        .execute(pool)
        .await
        .expect("seeded history row");
}

/// The type is interned through the dictionary inside the same statement, so
/// the catalogue row carries the key the broker's own write path would have
/// minted for it.
const SEED_TEMPORAL_ENTITY_SQL: &str = r#"
WITH type_name AS (
    INSERT INTO entity_type_names (name) VALUES ($2)
    ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name
    RETURNING id
)
INSERT INTO temporal_entities (context_space_key, created_at, modified_at, id, type_ids)
SELECT (SELECT context_space_key FROM context_spaces WHERE kind = 'live'), NOW(), NOW(), $1, ARRAY[type_name.id] FROM type_name
RETURNING entity_key
"#;

const ATTRIBUTE_NAME_ID_SQL: &str = r#"
INSERT INTO attribute_names (name) VALUES ($1)
ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name
RETURNING id
"#;

const SEED_STREAM_SQL: &str = r#"
INSERT INTO temporal_attribute_streams (context_space_key, temporal_entity_key, attribute_name_id)
SELECT context_space_key, entity_key, $2 FROM temporal_entities WHERE entity_key = $1
RETURNING stream_key
"#;

const SEED_WRITE_RECORD_SQL: &str = r#"
INSERT INTO write_records (write_id, recorded_at, accepted_at) VALUES ($1, $2, $2)
"#;

const SEED_HISTORY_ROW_SQL: &str = r#"
INSERT INTO attribute_history
    (recorded_at, context_space_key, write_id, stream_key, write_ordinal, attribute_kind, value_kind,
     generated_instance_uuid, observed_at, created_at, modified_at, value_number)
SELECT $1, context_space_key, $2, stream_key, 0, 1, 1, $4, $5, $1, $1, $6
FROM temporal_attribute_streams WHERE stream_key = $3
"#;
