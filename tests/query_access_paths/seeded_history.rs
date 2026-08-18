//! A production-shaped attribute-history corpus written straight into the
//! rebuilt temporal relations.
//!
//! Semantic-time plans need enough rows per stream for the planner to prefer an
//! index over a chunk scan, and they need every one of the four axes populated
//! with deliberately different instants. Writing that through the API would take
//! minutes per test, so the rows are written directly and removed again by the
//! harness prefix cleanup.

use sqlx::PgPool;

/// Streams written per seeded history corpus.
pub const SEEDED_STREAMS: i64 = 200;

/// Observations written per seeded stream.
pub const OBSERVATIONS_PER_STREAM: i64 = 100;

/// Expanded IRI of the seeded history attribute.
pub const HISTORY_ATTRIBUTE: &str = "https://example.test/query-access-paths/history-reading";

/// The seeded history corpus, and the identities a plan assertion binds.
pub struct SeededHistory {
    /// Stream selected by the narrow assertions.
    narrow_stream: i64,
    /// Temporal entity key every seeded stream hangs off.
    entity_key: i64,
}

impl SeededHistory {
    /// The one stream a narrow selection addresses.
    pub const fn narrow_stream(&self) -> i64 {
        self.narrow_stream
    }

    /// The temporal entity every seeded stream belongs to.
    pub const fn entity_key(&self) -> i64 {
        self.entity_key
    }
}

/// Write the corpus and analyze the relations the plans read.
///
/// Every observation carries four distinct instants: `observedAt` on a one-hour
/// grid, `createdAt` one day earlier, `modifiedAt` one day later, and no
/// `deletedAt` except on the final observation of each stream, which is a
/// tombstone. That separation is what makes an axis answering from another
/// axis' column detectable.
pub async fn seed(pool: &PgPool, prefix: &str) -> SeededHistory {
    let attribute_name_id: i32 =
        sqlx::query_scalar("INSERT INTO attribute_names (name) VALUES ($1) ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING id")
            .bind(HISTORY_ATTRIBUTE)
            .fetch_one(pool)
            .await
            .expect("attribute dictionary insert must succeed");

    let entity_key: i64 = sqlx::query_scalar(
        "WITH type_name AS ( \
             INSERT INTO entity_type_names (name) VALUES ('https://example.test/query-access-paths/Historic') \
             ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING id) \
         INSERT INTO temporal_entities (created_at, modified_at, id, type_ids) \
         SELECT now(), now(), $1::text || ':history', ARRAY[type_name.id] FROM type_name \
         RETURNING entity_key",
    )
    .bind(prefix)
    .fetch_one(pool)
    .await
    .expect("temporal catalogue insert must succeed");

    let streams: Vec<i64> = sqlx::query_scalar(
        "INSERT INTO temporal_attribute_streams (temporal_entity_key, attribute_name_id, dataset_id) \
         SELECT $1, $2, 'urn:ngsi-ld:Dataset:' || lpad(g::text, 4, '0') \
         FROM generate_series(1, $3::bigint) AS g \
         RETURNING stream_key",
    )
    .bind(entity_key)
    .bind(attribute_name_id)
    .bind(SEEDED_STREAMS)
    .fetch_all(pool)
    .await
    .expect("stream corpus must be writable");

    sqlx::query(
        "INSERT INTO attribute_history \
         (recorded_at, write_id, stream_key, write_ordinal, attribute_kind, value_kind, \
          observed_at, created_at, modified_at, deleted_at, generated_instance_uuid, value_number) \
         SELECT now() - (g || ' hours')::interval, \
                gen_random_uuid(), s.stream_key, 0, 1, \
                CASE WHEN g = 1 THEN 0 ELSE 1 END, \
                TIMESTAMPTZ '2026-01-01 00:00:00Z' + (g || ' hours')::interval, \
                TIMESTAMPTZ '2026-01-01 00:00:00Z' + (g || ' hours')::interval - INTERVAL '1 day', \
                TIMESTAMPTZ '2026-01-01 00:00:00Z' + (g || ' hours')::interval + INTERVAL '1 day', \
                CASE WHEN g = 1 THEN TIMESTAMPTZ '2026-01-01 00:00:00Z' + (g || ' hours')::interval + INTERVAL '2 days' END, \
                gen_random_uuid(), \
                CASE WHEN g = 1 THEN NULL ELSE g::numeric END \
         FROM temporal_attribute_streams AS s \
         CROSS JOIN generate_series(1, $2::bigint) AS g \
         WHERE s.temporal_entity_key = $1",
    )
    .bind(entity_key)
    .bind(OBSERVATIONS_PER_STREAM)
    .execute(pool)
    .await
    .expect("history corpus must be writable");

    sqlx::query("ANALYZE attribute_history").execute(pool).await.expect("analyze must succeed");
    sqlx::query("ANALYZE temporal_attribute_streams")
        .execute(pool)
        .await
        .expect("analyze must succeed");

    SeededHistory {
        narrow_stream: *streams.first().unwrap_or_else(|| unreachable!("the corpus writes at least one stream")),
        entity_key,
    }
}
