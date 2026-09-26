//! A production-shaped corpus written straight into the schema, and the
//! `EXPLAIN` reader the plan assertions share.
//!
//! Access-path assertions need enough rows for the planner to prefer an index
//! over a sequential scan, and they need the statistics that decision reads.
//! Creating that corpus through the API would take minutes per test, so the
//! rows are written directly and cleared again by the exclusive serialized
//! reset that runs before each test in this suite.

use andromeda_test_fixtures::high_entropy_text::high_entropy_text;
use sqlx::{Executor, PgPool, Postgres};

/// Identifier prefix every resource this suite seeds carries.
///
/// The suite holds the exclusive serialized lease, which resets the tenant to a
/// clean slate before each test and serializes every holder, so one fixed
/// prefix suffices: no per-test uniqueness is needed, the seeded corpus is the
/// only data in the analyzed tables, and every unscoped `EXPLAIN` plan is
/// therefore deterministic.
pub const SEED_PREFIX: &str = "urn:ngsi-ld:test:query-access-paths";

/// Entities written per seeded corpus.
pub const SEEDED_ENTITIES: i64 = 20_000;

/// Type carried by every 997th seeded Entity, selective enough that an index
/// path is the cheaper plan.
pub const RARE_TYPE: &str = "https://example.test/query-access-paths/Rare";

/// Type carried by every other seeded Entity.
pub const COMMON_TYPE: &str = "https://example.test/query-access-paths/Building";

/// Expanded IRI of the seeded numeric attribute.
pub const READING_ATTRIBUTE: &str = "https://example.test/query-access-paths/reading";

/// Expanded IRI of the seeded `GeoProperty` attribute.
pub const ENTRANCE_ATTRIBUTE: &str = "https://example.test/query-access-paths/entrance";

/// Expanded IRI of the attribute every seeded Entity carries, so an `attrs`
/// term on it selects the whole corpus.
pub const CITY_ATTRIBUTE: &str = "https://example.test/query-access-paths/city";

/// Expanded IRI of the attribute every 1000th seeded Entity carries, selective
/// enough that an `attrs` term on it prefers the index.
pub const ALARM_ATTRIBUTE: &str = "https://example.test/query-access-paths/alarm";

/// Expanded IRI of the attribute whose seeded values are all longer than the
/// 2048-octet indexed bound, so every one of them is projected into
/// `value_long_text`.
pub const ESSAY_ATTRIBUTE: &str = "https://example.test/query-access-paths/essay";

/// Seeded Entities carrying an essay, the first of the corpus by ordinal.
///
/// A handful of overflow rows is cheaper to filter on the heap from the
/// partial B-tree than to probe through the trigram GIN, so the planner only
/// reaches for the trigram indexes once the overflow population is in the
/// thousands; this is comfortably past that crossover, which lies between one
/// and two thousand rows for this corpus.
const SEEDED_ESSAYS: u64 = 3_000;

/// Octets of every seeded essay: past the 2048-octet bound, short of anything
/// that would make the corpus slow to write.
const ESSAY_LENGTH: usize = 3000;

/// Ordinal of the essay the probe substring is cut from.
const PROBED_ESSAY: usize = 7;

/// Characters of the probe substring, enough trigrams that the pattern is
/// selective.
const PROBE_LENGTH: usize = 16;

/// The dictionary identifiers of a seeded corpus, resolved once so a plan
/// assertion binds an integer instead of joining the dictionary.
pub struct SeededCorpus {
    /// Identifier prefix every seeded Entity carries.
    prefix: String,
    /// Dictionary identifier of [`READING_ATTRIBUTE`].
    reading: i32,
    /// Dictionary identifier of [`ENTRANCE_ATTRIBUTE`].
    entrance: i32,
    /// Type dictionary identifier of [`RARE_TYPE`].
    rare_type: i32,
    /// Type dictionary identifier of [`COMMON_TYPE`].
    common_type: i32,
    /// Dictionary identifier of [`CITY_ATTRIBUTE`].
    city: i32,
    /// Dictionary identifier of [`ALARM_ATTRIBUTE`].
    alarm: i32,
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

    /// Dictionary identifier of the seeded `GeoProperty` attribute.
    pub const fn entrance(&self) -> i32 {
        self.entrance
    }

    /// Type dictionary identifier of the rare seeded type.
    pub const fn rare_type(&self) -> i32 {
        self.rare_type
    }

    /// Type dictionary identifier of the common seeded type.
    pub const fn common_type(&self) -> i32 {
        self.common_type
    }

    /// Dictionary identifier of the attribute every seeded Entity carries.
    pub const fn city(&self) -> i32 {
        self.city
    }

    /// Dictionary identifier of the rare seeded attribute.
    pub const fn alarm(&self) -> i32 {
        self.alarm
    }

    /// Identifier of one seeded Entity by ordinal.
    pub fn entity_id(&self, ordinal: usize) -> String {
        format!("{}:seed:{ordinal:06}", self.prefix)
    }
}

/// The overflow strings seeded beside a corpus, and the identities a plan
/// assertion binds.
pub struct SeededEssays {
    /// Dictionary identifier of [`ESSAY_ATTRIBUTE`].
    attribute: i32,
    /// A substring from the middle of one seeded essay, present in no other.
    probe: String,
}

impl SeededEssays {
    /// Dictionary identifier of the overflowing attribute.
    pub const fn attribute(&self) -> i32 {
        self.attribute
    }

    /// Borrow the substring a pattern over the essays probes for.
    pub fn probe(&self) -> &str {
        &self.probe
    }
}

/// Resolve, creating if necessary, the dictionary identifier of an attribute
/// IRI. The dictionary is deliberately never truncated by the harness, so a
/// repeated run reuses the same identifier.
pub async fn dictionary_key(pool: &PgPool, name: &str) -> i32 {
    sqlx::query_scalar("INSERT INTO attribute_names (name) VALUES ($1) ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING attribute_name_key")
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("attribute dictionary insert must succeed")
}

/// Resolve, creating if necessary, the type dictionary identifier of an entity
/// type IRI. Like the attribute dictionary, it is never truncated by the
/// harness, so a repeated run reuses the same identifier.
pub async fn type_dictionary_key(pool: &PgPool, name: &str) -> i32 {
    sqlx::query_scalar("INSERT INTO entity_type_names (name) VALUES ($1) ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING entity_type_name_key")
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("type dictionary insert must succeed")
}

/// Write the corpus and analyze the relations the plans read.
///
/// Every Entity lands in all three current-state relations: the identity row,
/// a document row carrying a payload with the city attribute (and, on every
/// 1000th Entity, the alarm attribute), and a search row carrying the type,
/// the attribute keys, the location and the scope. The projection receives a
/// numeric reading, a `GeoProperty` entrance and the city string per Entity.
pub async fn seed(pool: &PgPool, prefix: &str) -> SeededCorpus {
    let reading = dictionary_key(pool, READING_ATTRIBUTE).await;
    let entrance = dictionary_key(pool, ENTRANCE_ATTRIBUTE).await;
    let city = dictionary_key(pool, CITY_ATTRIBUTE).await;
    let alarm = dictionary_key(pool, ALARM_ATTRIBUTE).await;
    let rare_type = type_dictionary_key(pool, RARE_TYPE).await;
    let common_type = type_dictionary_key(pool, COMMON_TYPE).await;

    sqlx::query(
        "WITH seeded AS ( \
             SELECT g, $1::text || ':seed:' || lpad(g::text, 6, '0') AS id FROM generate_series(1, $4::bigint) AS g), \
         ins AS ( \
             INSERT INTO entities (id) SELECT id FROM seeded RETURNING entity_key, id), \
         docs AS ( \
             INSERT INTO entity_documents (entity_key, payload) \
             SELECT ins.entity_key, \
                    jsonb_build_array(jsonb_build_object('@id', ins.id, $7::text, to_jsonb('city-' || (seeded.g % 1000)::text)) \
                                      || CASE WHEN seeded.g % 1000 = 0 THEN jsonb_build_object($8::text, to_jsonb(true)) ELSE '{}'::jsonb END) \
             FROM ins JOIN seeded ON seeded.id = ins.id) \
         INSERT INTO entity_search (entity_key, entity_type_name_keys, attribute_name_keys, scope_paths, location) \
         SELECT ins.entity_key, \
                ARRAY[CASE WHEN seeded.g % 997 = 0 THEN $2::integer ELSE $3::integer END], \
                CASE WHEN seeded.g % 1000 = 0 THEN ARRAY[$5::integer, $6::integer] ELSE ARRAY[$5::integer] END, \
                ARRAY[('seed.' || (seeded.g % 997)::text)::ltree], \
                ST_SetSRID(ST_MakePoint(13.0 + (seeded.g % 1000) * 0.001, 52.0 + (seeded.g % 1000) * 0.001), 4326) \
         FROM ins JOIN seeded ON seeded.id = ins.id",
    )
    .bind(prefix)
    .bind(rare_type)
    .bind(common_type)
    .bind(SEEDED_ENTITIES)
    .bind(city)
    .bind(alarm)
    .bind(CITY_ATTRIBUTE)
    .bind(ALARM_ATTRIBUTE)
    .execute(pool)
    .await
    .expect("entity corpus must be writable");

    let like = format!("{prefix}:seed:%");
    sqlx::query(
        "INSERT INTO current_attribute_values \
         (entity_key, attribute_name_key, attribute_kind, value_kind, value_number) \
         SELECT e.entity_key, $1, 1, 1, (e.entity_key % 20000)::numeric \
         FROM entities AS e WHERE e.id LIKE $2::text",
    )
    .bind(reading)
    .bind(&like)
    .execute(pool)
    .await
    .expect("numeric projection corpus must be writable");

    sqlx::query(
        "INSERT INTO current_attribute_values \
         (entity_key, attribute_name_key, attribute_kind, value_kind, value_geometry) \
         SELECT e.entity_key, $1, 3, 4, \
                ST_SetSRID(ST_MakePoint(13.0 + (e.entity_key % 1000) * 0.001, 52.0 + (e.entity_key % 1000) * 0.001), 4326) \
         FROM entities AS e WHERE e.id LIKE $2::text",
    )
    .bind(entrance)
    .bind(&like)
    .execute(pool)
    .await
    .expect("geometry projection corpus must be writable");

    sqlx::query(
        "INSERT INTO current_attribute_values \
         (entity_key, attribute_name_key, attribute_kind, value_kind, value_string) \
         SELECT e.entity_key, $1, 1, 2, 'city-' || (e.entity_key % 1000)::text \
         FROM entities AS e WHERE e.id LIKE $2::text",
    )
    .bind(city)
    .bind(&like)
    .execute(pool)
    .await
    .expect("string projection corpus must be writable");

    analyze(pool).await;

    SeededCorpus {
        prefix: prefix.to_owned(),
        reading,
        entrance,
        rare_type,
        common_type,
        city,
        alarm,
    }
}

/// Project one high-entropy essay past the indexed bound onto each of the
/// first [`SEEDED_ESSAYS`] Entities of a seeded corpus, and re-analyze.
///
/// The essays land in `value_long_text` under value kind 2, the placement the
/// writer gives every String longer than 2048 UTF-8 octets, so the overflow
/// indexes hold exactly these rows.
pub async fn seed_essays(pool: &PgPool, corpus: &SeededCorpus) -> SeededEssays {
    let attribute = dictionary_key(pool, ESSAY_ATTRIBUTE).await;
    let essays: Vec<String> = (0..SEEDED_ESSAYS).map(|seed| high_entropy_text(ESSAY_LENGTH, seed)).collect();
    let probe = essays
        .get(PROBED_ESSAY)
        .and_then(|essay| essay.get(ESSAY_LENGTH / 2..ESSAY_LENGTH / 2 + PROBE_LENGTH))
        .expect("the probed essay is seeded and ASCII")
        .to_owned();

    sqlx::query(
        "INSERT INTO current_attribute_values \
         (entity_key, attribute_name_key, attribute_kind, value_kind, value_long_text) \
         SELECT e.entity_key, $1, 1, 2, essay.body \
         FROM unnest($2::text[]) WITH ORDINALITY AS essay(body, ordinal) \
         JOIN entities AS e ON e.id = $3::text || ':seed:' || lpad(essay.ordinal::text, 6, '0')",
    )
    .bind(attribute)
    .bind(&essays)
    .bind(corpus.prefix())
    .execute(pool)
    .await
    .expect("overflow projection corpus must be writable");

    analyze(pool).await;

    SeededEssays { attribute, probe }
}

/// Refresh the statistics the planner reads for every relation the plans touch.
pub async fn analyze(pool: &PgPool) {
    for statement in [
        "ANALYZE entities",
        "ANALYZE entity_documents",
        "ANALYZE entity_search",
        "ANALYZE current_attribute_values",
    ] {
        sqlx::query(statement).execute(pool).await.expect("analyze must succeed");
    }
}

/// The planner paths a rolled-back `EXPLAIN` leaves open.
#[derive(Clone, Copy)]
pub enum PlannerPaths {
    /// Every path the planner considers by default.
    Unrestricted,
    /// Sequential scans priced out, for a corpus too small for the planner to
    /// prefer an index on cost alone: the plan then shows whether the index can
    /// answer the predicate at all.
    WithoutSequentialScans,
}

/// Run `EXPLAIN (ANALYZE, BUFFERS)` on one executor and return the plan as one
/// text block.
pub async fn explain<'e, E>(executor: E, statement: &str) -> String
where
    E: Executor<'e, Database = Postgres>,
{
    let lines: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("EXPLAIN (ANALYZE, BUFFERS) {statement}")))
        .fetch_all(executor)
        .await
        .expect("the plan must be readable");
    lines.join("\n")
}

/// Run `EXPLAIN (ANALYZE, BUFFERS)` inside a transaction that is rolled back
/// afterwards, so a planner setting stays local to the one statement and a
/// planned write leaves nothing behind.
pub async fn explain_rolled_back(pool: &PgPool, paths: PlannerPaths, statement: &str) -> String {
    let mut transaction = pool.begin().await.expect("the plan transaction must open");
    match paths {
        PlannerPaths::Unrestricted => (),
        PlannerPaths::WithoutSequentialScans => {
            sqlx::query("SET LOCAL enable_seqscan = off")
                .execute(&mut *transaction)
                .await
                .expect("the planner setting must apply");
        }
    }
    let plan = explain(&mut *transaction, statement).await;
    transaction.rollback().await.expect("the plan transaction must roll back");
    plan
}
