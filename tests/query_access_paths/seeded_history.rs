//! A production-shaped attribute-history corpus written straight into the
//! rebuilt temporal relations.
//!
//! Semantic-time plans need enough rows per stream for the planner to prefer an
//! index over a chunk scan, and they need every one of the four axes populated
//! with deliberately different instants. Writing that through the API would take
//! minutes per test, so the rows are written directly and cleared again by the
//! exclusive serialized reset that runs before each test in this suite.

use crate::seeded_corpus::{dictionary_key, type_dictionary_key};
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
    let attribute_name_key: i32 =
        sqlx::query_scalar("INSERT INTO attribute_names (name) VALUES ($1) ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING attribute_name_key")
            .bind(HISTORY_ATTRIBUTE)
            .fetch_one(pool)
            .await
            .expect("attribute dictionary insert must succeed");

    let entity_key: i64 = sqlx::query_scalar(
        "WITH type_name AS ( \
             INSERT INTO entity_type_names (name) VALUES ('https://example.test/query-access-paths/Historic') \
             ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING entity_type_name_key) \
         INSERT INTO temporal_entities (created_at, modified_at, id, entity_type_name_keys) \
         SELECT now(), now(), $1::text || ':history', ARRAY[type_name.entity_type_name_key] FROM type_name \
         RETURNING temporal_entity_key",
    )
    .bind(prefix)
    .fetch_one(pool)
    .await
    .expect("temporal catalogue insert must succeed");

    let streams: Vec<i64> = sqlx::query_scalar(
        "INSERT INTO temporal_attribute_streams (temporal_entity_key, attribute_name_key, dataset_id) \
         SELECT $1, $2, 'urn:ngsi-ld:Dataset:' || lpad(g::text, 4, '0') \
         FROM generate_series(1, $3::bigint) AS g \
         RETURNING stream_key",
    )
    .bind(entity_key)
    .bind(attribute_name_key)
    .bind(SEEDED_STREAMS)
    .fetch_all(pool)
    .await
    .expect("stream corpus must be writable");

    sqlx::query(
        "INSERT INTO attribute_history \
         (recorded_at, write_id, stream_key, write_ordinal, attribute_kind, value_kind, \
          observed_at, created_at, modified_at, deleted_at, generated_instance_id, value_number) \
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
        narrow_stream: *streams.first().expect("the corpus writes at least one stream"),
        entity_key,
    }
}

/// Temporal Entities written per windowed corpus, each with one stream of the
/// history attribute.
pub const WINDOWED_ENTITIES: i64 = 300;

/// Hours between the observation in effect when the queried window opens and
/// the end of the seeded history, so the history before the window grows with
/// the corpus while the window itself stays the same size.
const WINDOW_TAIL_HOURS: i64 = 100;

/// Minutes after the observation in effect at which the queried window opens:
/// no observation sits at the start itself, which would otherwise be the
/// window's own (clause 4.11 opens it inclusively).
const WINDOW_OPENING_MINUTES: i64 = 30;

/// Minutes after the observation in effect at which its stream's tombstone
/// is placed, on the one stream that holds one.
const TOMBSTONE_MINUTES: i64 = 15;

/// Minutes the queried window spans.
const WINDOW_MINUTES: i64 = 600;

/// A corpus a temporal `q` plan reads: one stream per temporal Entity, an
/// hourly observation per stream, and one stream whose value was deleted just
/// before the window opens.
pub struct WindowedHistory {
    /// Dictionary identifier of [`HISTORY_ATTRIBUTE`].
    attribute: i32,
    /// Observations written per stream.
    observations: i64,
    /// Identifier of the temporal Entity whose stream holds the tombstone.
    tombstoned: String,
}

impl WindowedHistory {
    /// Dictionary identifier of the seeded history attribute.
    pub const fn attribute(&self) -> i32 {
        self.attribute
    }

    /// The hour, counted from the seeded origin, of the observation every
    /// stream holds when the window opens; its value is the hour too.
    pub const fn value_in_effect(&self) -> i64 {
        self.observations - WINDOW_TAIL_HOURS
    }

    /// The minute, counted from the seeded origin, the queried window opens at.
    pub const fn window_start_minute(&self) -> i64 {
        self.value_in_effect() * 60 + WINDOW_OPENING_MINUTES
    }

    /// The minute the queried window closes at, exclusively.
    pub const fn window_end_minute(&self) -> i64 {
        self.window_start_minute() + WINDOW_MINUTES
    }

    /// Identifier of the temporal Entity whose value was deleted before the
    /// window opened, so it holds nothing in effect there.
    pub fn tombstoned(&self) -> &str {
        &self.tombstoned
    }
}

/// Write the windowed corpus and analyze the relations its plans read.
///
/// Observation `g` of every stream sits at hour `g` after `2026-01-01` with the
/// value `g`, and the window opens half an hour after one of them. The first
/// Entity's stream also holds a deletion with no `observedAt` between that
/// observation and the start, which is what that stream holds in effect at the
/// start (clause 4.5.7). Every row is recorded
/// within one hour of one fixed instant, so the corpus lands in one chunk
/// however long its history is: a growing chunk count would grow every
/// stream-led probe for a reason that is not the probe's.
pub async fn seed_window_and_prior(pool: &PgPool, prefix: &str, observations: i64) -> WindowedHistory {
    let attribute = dictionary_key(pool, HISTORY_ATTRIBUTE).await;
    let type_key = type_dictionary_key(pool, "https://example.test/query-access-paths/Meter").await;

    sqlx::query(
        "WITH meters AS ( \
             INSERT INTO temporal_entities (created_at, modified_at, id, entity_type_name_keys) \
             SELECT now(), now(), $1::text || ':meter:' || lpad(g::text, 4, '0'), ARRAY[$2::integer] \
             FROM generate_series(1, $3::bigint) AS g \
             RETURNING temporal_entity_key) \
         INSERT INTO temporal_attribute_streams (temporal_entity_key, attribute_name_key) \
         SELECT temporal_entity_key, $4 FROM meters",
    )
    .bind(prefix)
    .bind(type_key)
    .bind(WINDOWED_ENTITIES)
    .bind(attribute)
    .execute(pool)
    .await
    .expect("the windowed catalogue must be writable");

    sqlx::query(
        "INSERT INTO attribute_history \
         (recorded_at, write_id, stream_key, write_ordinal, attribute_kind, value_kind, observed_at, generated_instance_id, value_number) \
         SELECT TIMESTAMPTZ '2026-01-04 12:00:00Z' + (g || ' seconds')::interval, \
                gen_random_uuid(), s.stream_key, 0, 1, 1, \
                TIMESTAMPTZ '2026-01-01 00:00:00Z' + (g || ' hours')::interval, \
                gen_random_uuid(), g::numeric \
         FROM temporal_attribute_streams AS s \
         CROSS JOIN generate_series(1, $2::bigint) AS g \
         WHERE s.attribute_name_key = $1",
    )
    .bind(attribute)
    .bind(observations)
    .execute(pool)
    .await
    .expect("the windowed history must be writable");

    let tombstoned = format!("{prefix}:meter:0001");
    sqlx::query(
        "INSERT INTO attribute_history \
         (recorded_at, write_id, stream_key, write_ordinal, attribute_kind, value_kind, deleted_at, generated_instance_id) \
         SELECT TIMESTAMPTZ '2026-01-04 12:00:00Z', gen_random_uuid(), s.stream_key, 0, 1, 0, \
                TIMESTAMPTZ '2026-01-01 00:00:00Z' + ($3::bigint || ' minutes')::interval, \
                gen_random_uuid() \
         FROM temporal_attribute_streams AS s \
         JOIN temporal_entities AS e ON e.temporal_entity_key = s.temporal_entity_key \
         WHERE e.id = $1 AND s.attribute_name_key = $2",
    )
    .bind(&tombstoned)
    .bind(attribute)
    .bind((observations - WINDOW_TAIL_HOURS) * 60 + TOMBSTONE_MINUTES)
    .execute(pool)
    .await
    .expect("the tombstone must be writable");

    for statement in ["ANALYZE attribute_history", "ANALYZE temporal_attribute_streams", "ANALYZE temporal_entities"] {
        sqlx::query(statement).execute(pool).await.expect("analyze must succeed");
    }

    WindowedHistory {
        attribute,
        observations,
        tombstoned,
    }
}

/// Remove a windowed corpus written under `prefix` and vacuum what it leaves
/// behind, so a corpus seeded next is read without the first one's dead rows.
pub async fn remove_windowed_history(pool: &PgPool, prefix: &str) {
    let meters = format!("{prefix}:meter:%");
    for statement in [
        "DELETE FROM attribute_history AS ah USING temporal_attribute_streams AS s, temporal_entities AS e \
         WHERE ah.stream_key = s.stream_key AND s.temporal_entity_key = e.temporal_entity_key AND e.id LIKE $1",
        "DELETE FROM temporal_attribute_streams AS s USING temporal_entities AS e \
         WHERE s.temporal_entity_key = e.temporal_entity_key AND e.id LIKE $1",
        "DELETE FROM temporal_entities WHERE id LIKE $1",
    ] {
        sqlx::query(statement)
            .bind(&meters)
            .execute(pool)
            .await
            .expect("the windowed corpus must be removable");
    }
    for statement in [
        "VACUUM ANALYZE attribute_history",
        "VACUUM ANALYZE temporal_attribute_streams",
        "VACUUM ANALYZE temporal_entities",
    ] {
        sqlx::query(statement).execute(pool).await.expect("vacuum must succeed");
    }
}
