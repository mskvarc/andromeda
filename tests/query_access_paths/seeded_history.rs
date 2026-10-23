//! A production-shaped attribute-history corpus written straight into the
//! rebuilt temporal relations.
//!
//! Semantic-time plans need enough rows per stream for the planner to prefer an
//! index over a chunk scan, and they need every one of the four axes populated
//! with deliberately different instants. Writing that through the API would take
//! minutes per test, so the rows are written directly and cleared again by the
//! exclusive serialized reset that runs before each test in this suite.

use andromeda_postgresql_schema::{
    current::Entities,
    temporal::{AttributeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    catalog_function::{generate_series, lpad, random_uuid},
    clock::now,
    command::Command,
    interval::make_interval_secs,
    literal::text_literal,
    maintenance::{MaintenanceSchema, analyze, vacuum_analyze},
    pg_array::array_literal,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    statement::Statement,
};
use andromeda_test_harness::dictionary_seed::{attribute_name_key, entity_type_name_key};
use chrono::{DateTime, Utc};
use sea_query::{Alias, CaseStatement, CommonTableExpression, Expr, ExprTrait, IntoIden, Query, SimpleExpr, WithClause};
use sqlx::PgPool;

/// Streams written per seeded history corpus.
pub const SEEDED_STREAMS: i64 = 200;

/// Observations written per seeded stream.
pub const OBSERVATIONS_PER_STREAM: i64 = 100;

/// Expanded IRI of the seeded history attribute.
pub const HISTORY_ATTRIBUTE: &str = "https://example.test/query-access-paths/history-reading";

/// Seconds in a day.
const DAY_SECONDS: f64 = 86_400.0;

/// The instant the seeded observations count their hours from.
fn history_origin() -> DateTime<Utc> {
    "2026-01-01T00:00:00Z".parse().expect("the origin is a valid instant")
}

/// The instant the windowed corpus records every row at, an hour of seconds
/// wide.
fn windowed_recording() -> DateTime<Utc> {
    "2026-01-04T12:00:00Z".parse().expect("the recording instant is valid")
}

/// `(count || unit)::interval` for the column `count`, `unit` spelled with its
/// leading space.
fn counted_interval(count: Expr, unit: &str) -> SimpleExpr {
    cast(count.concat_text(text_literal(unit)), PostgresType::Interval)
}

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

/// Refresh the planner statistics of the three temporal relations.
async fn analyze_temporal(pool: &PgPool) {
    for relation in [
        AttributeHistory::Table.into_iden(),
        TemporalAttributeStreams::Table.into_iden(),
        Entities::Table.into_iden(),
    ] {
        analyze(MaintenanceSchema::Public, relation)
            .to_query()
            .execute(pool)
            .await
            .expect("analyze must succeed");
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
    let history_attribute_key = attribute_name_key(pool, HISTORY_ATTRIBUTE)
        .await
        .expect("attribute dictionary seed must succeed");
    let historic_type_key = entity_type_name_key(pool, "https://example.test/query-access-paths/Historic")
        .await
        .expect("type dictionary seed must succeed");

    let entity_key = insert_history_entity(pool, prefix, historic_type_key).await;
    let streams = insert_streams(pool, entity_key, history_attribute_key).await;
    insert_observations(pool, entity_key).await;

    for relation in [AttributeHistory::Table.into_iden(), TemporalAttributeStreams::Table.into_iden()] {
        analyze(MaintenanceSchema::Public, relation)
            .to_query()
            .execute(pool)
            .await
            .expect("analyze must succeed");
    }

    SeededHistory {
        narrow_stream: *streams.first().expect("the corpus writes at least one stream"),
        entity_key,
    }
}

/// Insert the one temporal Entity of the corpus and return its key.
async fn insert_history_entity(pool: &PgPool, prefix: &str, historic_type_key: i32) -> i64 {
    Query::insert()
        .into_table(Entities::Table)
        .columns([
            Entities::EvolutionCreatedAt,
            Entities::EvolutionModifiedAt,
            Entities::Id,
            Entities::EvolutionTypeNameKeys,
        ])
        .values([
            now(),
            now(),
            cast(Expr::val(prefix), PostgresType::Text).concat_text(text_literal(":history")),
            array_literal([cast(Expr::val(historic_type_key), PostgresType::Integer)]),
        ])
        .expect("one value per column")
        .returning_col(Entities::EntityKey)
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("temporal catalogue insert must succeed")
}

/// Insert the streams of the history attribute for the Entity and return their
/// keys.
async fn insert_streams(pool: &PgPool, entity_key: i64, history_attribute_key: i32) -> Vec<i64> {
    let series = || Expr::col(Alias::new("g"));
    Query::insert()
        .into_table(TemporalAttributeStreams::Table)
        .columns([
            TemporalAttributeStreams::EntityKey,
            TemporalAttributeStreams::AttributeNameKey,
            TemporalAttributeStreams::DatasetId,
        ])
        .select_from(
            Query::select()
                .column((Alias::new("e"), Entities::EntityKey))
                .expr(Expr::val(history_attribute_key))
                .expr(text_literal("urn:ngsi-ld:Dataset:").concat_text(lpad(cast(series(), PostgresType::Text), Expr::val(4), text_literal("0"))))
                .from_as(Entities::Table, Alias::new("e"))
                .from_function(
                    generate_series(Expr::val(1), cast(Expr::val(SEEDED_STREAMS), PostgresType::Bigint)),
                    Alias::new("g"),
                )
                .and_where(Expr::col((Alias::new("e"), Entities::EntityKey)).eq(entity_key))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .returning_col(TemporalAttributeStreams::StreamKey)
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("stream corpus must be writable")
}

/// Insert the observations of every stream of the Entity.
async fn insert_observations(pool: &PgPool, entity_key: i64) {
    let series = || Expr::col(Alias::new("g"));
    let observed = || Expr::val(history_origin()).add(counted_interval(series(), " hours"));
    let first_observation = || series().eq(1);
    Query::insert()
        .into_table(AttributeHistory::Table)
        .columns([
            AttributeHistory::RecordedAt,
            AttributeHistory::WriteId,
            AttributeHistory::StreamKey,
            AttributeHistory::WriteOrdinal,
            AttributeHistory::AttributeKind,
            AttributeHistory::ValueKind,
            AttributeHistory::ObservedAt,
            AttributeHistory::CreatedAt,
            AttributeHistory::ModifiedAt,
            AttributeHistory::DeletedAt,
            AttributeHistory::GeneratedInstanceId,
            AttributeHistory::ValueNumber,
        ])
        .select_from(
            Query::select()
                .expr(Expr::expr(now()).sub(counted_interval(series(), " hours")))
                .expr(random_uuid())
                .column((Alias::new("s"), TemporalAttributeStreams::StreamKey))
                .expr(Expr::val(0))
                .expr(Expr::val(1))
                .expr(CaseStatement::new().case(first_observation(), Expr::val(0)).finally(Expr::val(1)))
                .expr(observed())
                .expr(Expr::expr(observed()).sub(make_interval_secs(Expr::val(DAY_SECONDS))))
                .expr(Expr::expr(observed()).add(make_interval_secs(Expr::val(DAY_SECONDS))))
                .expr(CaseStatement::new().case(
                    first_observation(),
                    Expr::expr(observed()).add(make_interval_secs(Expr::val(2.0 * DAY_SECONDS))),
                ))
                .expr(random_uuid())
                .expr(
                    CaseStatement::new()
                        .case(first_observation(), Expr::null())
                        .finally(cast(series(), PostgresType::Numeric)),
                )
                .from_as(TemporalAttributeStreams::Table, Alias::new("s"))
                .from_function(
                    generate_series(Expr::val(1), cast(Expr::val(OBSERVATIONS_PER_STREAM), PostgresType::Bigint)),
                    Alias::new("g"),
                )
                .and_where(Expr::col((Alias::new("s"), TemporalAttributeStreams::EntityKey)).eq(entity_key))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_query()
        .execute(pool)
        .await
        .expect("history corpus must be writable");
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
    let attribute = attribute_name_key(pool, HISTORY_ATTRIBUTE)
        .await
        .expect("attribute dictionary seed must succeed");
    let type_key = entity_type_name_key(pool, "https://example.test/query-access-paths/Meter")
        .await
        .expect("type dictionary seed must succeed");

    insert_meter_catalogue(pool, prefix, type_key, attribute).await;
    insert_windowed_observations(pool, attribute, observations).await;

    let tombstoned = format!("{prefix}:meter:0001");
    let tombstone_minutes = (observations - WINDOW_TAIL_HOURS) * 60 + TOMBSTONE_MINUTES;
    insert_tombstone(pool, &tombstoned, tombstone_minutes, attribute).await;

    analyze_temporal(pool).await;

    WindowedHistory {
        attribute,
        observations,
        tombstoned,
    }
}

/// Insert the meters and one stream of `attribute` for each.
async fn insert_meter_catalogue(pool: &PgPool, prefix: &str, type_key: i32, attribute: i32) {
    let series = || Expr::col(Alias::new("g"));
    let meters = Query::insert()
        .into_table(Entities::Table)
        .columns([
            Entities::EvolutionCreatedAt,
            Entities::EvolutionModifiedAt,
            Entities::Id,
            Entities::EvolutionTypeNameKeys,
        ])
        .select_from(
            Query::select()
                .expr(now())
                .expr(now())
                .expr(
                    cast(Expr::val(prefix), PostgresType::Text)
                        .concat_text(text_literal(":meter:"))
                        .concat_text(lpad(cast(series(), PostgresType::Text), Expr::val(4), text_literal("0"))),
                )
                .expr(array_literal([cast(Expr::val(type_key), PostgresType::Integer)]))
                .from_function(
                    generate_series(Expr::val(1), cast(Expr::val(WINDOWED_ENTITIES), PostgresType::Bigint)),
                    Alias::new("g"),
                )
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .returning(Query::returning().columns([Entities::EntityKey, Entities::Id]))
        .to_owned();
    let streams = Query::insert()
        .into_table(TemporalAttributeStreams::Table)
        .columns([TemporalAttributeStreams::EntityKey, TemporalAttributeStreams::AttributeNameKey])
        .select_from(
            Query::select()
                .column(Entities::EntityKey)
                .expr(Expr::val(attribute))
                .from(Alias::new("meters"))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_owned();
    WithClause::new()
        .cte(CommonTableExpression::new().table_name(Alias::new("meters")).query(meters).to_owned())
        .to_owned()
        .query(streams)
        .to_query()
        .execute(pool)
        .await
        .expect("the windowed catalogue must be writable");
}

/// Insert `observations` numeric observations into every stream of `attribute`.
async fn insert_windowed_observations(pool: &PgPool, attribute: i32, observations: i64) {
    let series = || Expr::col(Alias::new("g"));
    Query::insert()
        .into_table(AttributeHistory::Table)
        .columns([
            AttributeHistory::RecordedAt,
            AttributeHistory::WriteId,
            AttributeHistory::StreamKey,
            AttributeHistory::WriteOrdinal,
            AttributeHistory::AttributeKind,
            AttributeHistory::ValueKind,
            AttributeHistory::ObservedAt,
            AttributeHistory::GeneratedInstanceId,
            AttributeHistory::ValueNumber,
        ])
        .select_from(
            Query::select()
                .expr(Expr::val(windowed_recording()).add(counted_interval(series(), " seconds")))
                .expr(random_uuid())
                .column((Alias::new("s"), TemporalAttributeStreams::StreamKey))
                .expr(Expr::val(0))
                .expr(Expr::val(1))
                .expr(Expr::val(1))
                .expr(Expr::val(history_origin()).add(counted_interval(series(), " hours")))
                .expr(random_uuid())
                .expr(cast(series(), PostgresType::Numeric))
                .from_as(TemporalAttributeStreams::Table, Alias::new("s"))
                .from_function(
                    generate_series(Expr::val(1), cast(Expr::val(observations), PostgresType::Bigint)),
                    Alias::new("g"),
                )
                .and_where(Expr::col((Alias::new("s"), TemporalAttributeStreams::AttributeNameKey)).eq(attribute))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_query()
        .execute(pool)
        .await
        .expect("the windowed history must be writable");
}

/// Insert the deletion with no `observedAt` that the first meter's stream holds
/// after the observation in effect at the window start.
async fn insert_tombstone(pool: &PgPool, tombstoned: &str, tombstone_minutes: i64, attribute: i32) {
    Query::insert()
        .into_table(AttributeHistory::Table)
        .columns([
            AttributeHistory::RecordedAt,
            AttributeHistory::WriteId,
            AttributeHistory::StreamKey,
            AttributeHistory::WriteOrdinal,
            AttributeHistory::AttributeKind,
            AttributeHistory::ValueKind,
            AttributeHistory::DeletedAt,
            AttributeHistory::GeneratedInstanceId,
        ])
        .select_from(
            Query::select()
                .expr(Expr::val(windowed_recording()))
                .expr(random_uuid())
                .column((Alias::new("s"), TemporalAttributeStreams::StreamKey))
                .expr(Expr::val(0))
                .expr(Expr::val(1))
                .expr(Expr::val(0))
                .expr(Expr::val(history_origin()).add(counted_interval(cast(Expr::val(tombstone_minutes), PostgresType::Bigint), " minutes")))
                .expr(random_uuid())
                .from_as(TemporalAttributeStreams::Table, Alias::new("s"))
                .inner_join(
                    Entities::Table,
                    Expr::col((Entities::Table, Entities::EntityKey)).equals((Alias::new("s"), TemporalAttributeStreams::EntityKey)),
                )
                .and_where(Expr::col((Entities::Table, Entities::Id)).eq(tombstoned))
                .and_where(Expr::col((Alias::new("s"), TemporalAttributeStreams::AttributeNameKey)).eq(attribute))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_query()
        .execute(pool)
        .await
        .expect("the tombstone must be writable");
}

/// Remove a windowed corpus written under `prefix` and vacuum what it leaves
/// behind, so a corpus seeded next is read without the first one's dead rows.
pub async fn remove_windowed_history(pool: &PgPool, prefix: &str) {
    let meters = format!("{prefix}:meter:%");
    let meter_keys = || {
        Query::select()
            .column(Entities::EntityKey)
            .from(Entities::Table)
            .and_where(Expr::col(Entities::Id).like(meters.as_str()))
            .to_owned()
    };
    Query::delete()
        .from_table(AttributeHistory::Table)
        .and_where(
            Expr::col(AttributeHistory::StreamKey).in_subquery(
                Query::select()
                    .column(TemporalAttributeStreams::StreamKey)
                    .from(TemporalAttributeStreams::Table)
                    .and_where(Expr::col(TemporalAttributeStreams::EntityKey).in_subquery(meter_keys()))
                    .to_owned(),
            ),
        )
        .to_query()
        .execute(pool)
        .await
        .expect("the windowed corpus must be removable");
    Query::delete()
        .from_table(TemporalAttributeStreams::Table)
        .and_where(Expr::col(TemporalAttributeStreams::EntityKey).in_subquery(meter_keys()))
        .to_query()
        .execute(pool)
        .await
        .expect("the windowed corpus must be removable");
    Query::delete()
        .from_table(Entities::Table)
        .and_where(Expr::col(Entities::Id).like(meters.as_str()))
        .to_query()
        .execute(pool)
        .await
        .expect("the windowed corpus must be removable");
    for relation in [
        AttributeHistory::Table.into_iden(),
        TemporalAttributeStreams::Table.into_iden(),
        Entities::Table.into_iden(),
    ] {
        vacuum_analyze(MaintenanceSchema::Public, relation)
            .to_query()
            .execute(pool)
            .await
            .expect("vacuum must succeed");
    }
}
