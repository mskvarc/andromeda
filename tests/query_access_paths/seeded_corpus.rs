//! A production-shaped corpus written straight into the schema, and the
//! `EXPLAIN` reader the plan assertions share.
//!
//! Access-path assertions need enough rows for the planner to prefer an index
//! over a sequential scan, and they need the statistics that decision reads.
//! Creating that corpus through the API would take minutes per test, so the
//! rows are written directly and removed again by the harness prefix cleanup.

use sqlx::PgPool;

/// Entities written per seeded corpus.
pub const SEEDED_ENTITIES: i64 = 20_000;

/// Type carried by every 997th seeded Entity, selective enough that an index
/// path is the cheaper plan.
pub const RARE_TYPE: &str = "https://example.test/query-access-paths/Rare";

/// Type carried by every other seeded Entity.
pub const COMMON_TYPE: &str = "https://example.test/query-access-paths/Building";

/// Expanded IRI of the seeded numeric attribute.
pub const READING_ATTRIBUTE: &str = "https://example.test/query-access-paths/reading";

/// Expanded IRI of the seeded GeoProperty attribute.
pub const ENTRANCE_ATTRIBUTE: &str = "https://example.test/query-access-paths/entrance";

/// The dictionary identifiers of a seeded corpus, resolved once so a plan
/// assertion binds an integer instead of joining the dictionary.
pub struct SeededCorpus {
    /// Identifier prefix every seeded Entity carries.
    prefix: String,
    /// Dictionary identifier of [`READING_ATTRIBUTE`].
    reading: i32,
    /// Dictionary identifier of [`ENTRANCE_ATTRIBUTE`].
    entrance: i32,
}

impl SeededCorpus {
    /// Borrow the shared identifier prefix.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Dictionary identifier of the seeded numeric attribute.
    pub const fn reading(&self) -> i32 {
        self.reading
    }

    /// Dictionary identifier of the seeded GeoProperty attribute.
    pub const fn entrance(&self) -> i32 {
        self.entrance
    }

    /// Identifier of one seeded Entity by ordinal.
    pub fn entity_id(&self, ordinal: usize) -> String {
        format!("{}:seed:{ordinal:06}", self.prefix)
    }
}

/// Resolve, creating if necessary, the dictionary identifier of an attribute
/// IRI. The dictionary is deliberately never truncated by the harness, so a
/// repeated run reuses the same identifier.
async fn dictionary_id(pool: &PgPool, name: &str) -> i32 {
    sqlx::query_scalar("INSERT INTO attribute_names (name) VALUES ($1) ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING id")
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("attribute dictionary insert must succeed")
}

/// Write the corpus and analyze the two relations the plans read.
pub async fn seed(pool: &PgPool, prefix: &str) -> SeededCorpus {
    let reading = dictionary_id(pool, READING_ATTRIBUTE).await;
    let entrance = dictionary_id(pool, ENTRANCE_ATTRIBUTE).await;

    sqlx::query(
        "INSERT INTO entities (id, types, payload, location, scope_paths) \
         SELECT $1::text || ':seed:' || lpad(g::text, 6, '0'), \
                ARRAY[CASE WHEN g % 997 = 0 THEN $2::text ELSE $3::text END], \
                jsonb_build_array(jsonb_build_object('@id', $1::text || ':seed:' || lpad(g::text, 6, '0'), \
                                                     'https://example.test/query-access-paths/city', \
                                                     to_jsonb('city-' || (g % 1000)::text))), \
                ST_SetSRID(ST_MakePoint(13.0 + (g % 1000) * 0.001, 52.0 + (g % 1000) * 0.001), 4326), \
                ARRAY[('seed.' || (g % 997)::text)::ltree] \
         FROM generate_series(1, $4::bigint) AS g",
    )
    .bind(prefix)
    .bind(RARE_TYPE)
    .bind(COMMON_TYPE)
    .bind(SEEDED_ENTITIES)
    .execute(pool)
    .await
    .expect("entity corpus must be writable");

    let like = format!("{prefix}:seed:%");
    sqlx::query(
        "INSERT INTO current_attribute_values \
         (entity_key, modified_at, attribute_name_id, attribute_kind, value_kind, value_number) \
         SELECT e.entity_key, now(), $1, 1, 1, (e.entity_key % 20000)::numeric \
         FROM entities AS e WHERE e.id LIKE $2::text",
    )
    .bind(reading)
    .bind(&like)
    .execute(pool)
    .await
    .expect("numeric projection corpus must be writable");

    sqlx::query(
        "INSERT INTO current_attribute_values \
         (entity_key, modified_at, attribute_name_id, attribute_kind, value_kind, value_geo) \
         SELECT e.entity_key, now(), $1, 3, 4, \
                ST_SetSRID(ST_MakePoint(13.0 + (e.entity_key % 1000) * 0.001, 52.0 + (e.entity_key % 1000) * 0.001), 4326) \
         FROM entities AS e WHERE e.id LIKE $2::text",
    )
    .bind(entrance)
    .bind(&like)
    .execute(pool)
    .await
    .expect("geometry projection corpus must be writable");

    analyze(pool).await;

    SeededCorpus {
        prefix: prefix.to_owned(),
        reading,
        entrance,
    }
}

/// Refresh the statistics the planner reads for both relations.
pub async fn analyze(pool: &PgPool) {
    for statement in ["ANALYZE entities", "ANALYZE current_attribute_values"] {
        sqlx::query(statement).execute(pool).await.expect("analyze must succeed");
    }
}

/// Run `EXPLAIN (ANALYZE, BUFFERS)` and return the plan as one text block.
pub async fn explain(pool: &PgPool, statement: &str) -> String {
    let lines: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("EXPLAIN (ANALYZE, BUFFERS) {statement}")))
        .fetch_all(pool)
        .await
        .expect("the plan must be readable");
    lines.join("\n")
}
