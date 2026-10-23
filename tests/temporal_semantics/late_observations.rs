//! Late observations.
//!
//! An observation that arrives long after it was made is written now: the
//! hypertable partitions on the operational write clock, not on the semantic
//! axis. A one-day, one-month or one-year-late observation therefore stays in
//! the ingestion chunk the broker is currently writing, while remaining exactly
//! selectable on its own semantic axis.

use crate::{attribute_iri, client, create_temporal_entity, database, get_temporal_entity, rendered_instances, stored_history::stored_attribute_history};
use andromeda_ngsi_ld_lexicon::time::rendering::instant_text;
use andromeda_postgresql_schema::{
    current::Entities,
    temporal::{AttributeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{aggregate::count_distinct, statement::Statement};
use andromeda_test_harness::environment::TestEnvironment;
use chrono::{DateTime, TimeDelta, Utc};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use sea_query::{Alias, Expr, ExprTrait, Query};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};

/// The lateness of each replayed observation, in days behind the write instant.
const LATENESS_DAYS: [i64; 4] = [0, 1, 30, 365];

/// Render one instant in the spelling an NGSI-LD `observedAt` takes.
fn observation_instant(instant: DateTime<Utc>) -> String {
    instant_text(instant)
}

/// Count the chunks the history of one Entity occupies.
///
/// Every chunk of a hypertable is its own relation, so the number of distinct
/// `tableoid` values is the number of chunks the rows landed in.
async fn occupied_chunks(pool: &PgPool, entity_id: &str) -> i64 {
    Query::select()
        .expr_as(
            count_distinct(Expr::col((AttributeHistory::Table, Alias::new("tableoid")))),
            Alias::new("chunks"),
        )
        .from(AttributeHistory::Table)
        .inner_join(
            TemporalAttributeStreams::Table,
            Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey)).equals((AttributeHistory::Table, AttributeHistory::StreamKey)),
        )
        .inner_join(
            Entities::Table,
            Expr::col((Entities::Table, Entities::EntityKey)).equals((TemporalAttributeStreams::Table, TemporalAttributeStreams::EntityKey)),
        )
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(entity_id))
        .to_query()
        .fetch_one(pool)
        .await
        .expect("occupied chunk count")
        .try_get("chunks")
        .expect("the chunks column decodes")
}

/// The configured range interval of the attribute-history hypertable, in microseconds.
async fn attribute_history_chunk_interval(pool: &PgPool) -> i64 {
    let dimension = |column: &str| (Alias::new("_timescaledb_catalog"), Alias::new("dimension"), Alias::new(column));
    let hypertable = |column: &str| (Alias::new("_timescaledb_catalog"), Alias::new("hypertable"), Alias::new(column));
    Query::select()
        .expr_as(Expr::col(dimension("interval_length")), Alias::new("interval_us"))
        .from((Alias::new("_timescaledb_catalog"), Alias::new("dimension")))
        .inner_join(
            (Alias::new("_timescaledb_catalog"), Alias::new("hypertable")),
            Expr::col(hypertable("id")).equals(dimension("hypertable_id")),
        )
        .and_where(Expr::col(hypertable("table_name")).eq("attribute_history"))
        .to_query()
        .fetch_one(pool)
        .await
        .expect("attribute history chunk interval")
        .try_get("interval_us")
        .expect("the interval_us column decodes")
}

/// Late arrivals are written into the current ingestion chunk and stay exactly
/// selectable on the semantic axis they declare.
#[tokio::test]
async fn temporal_semantics_late_observations_stay_in_the_ingestion_chunk_and_remain_exact() {
    let scope = TestEnvironment::from_suite("temporal_semantics").await.expect("isolated test environment");
    let http = client();
    let pool = database().await;
    let entity_id = scope.id("Sensor:late_observations");
    let now = Utc::now();

    let instances = LATENESS_DAYS
        .iter()
        .map(|days| {
            json!({
                "type": "Property",
                "value": *days,
                "observedAt": observation_instant(now - TimeDelta::days(*days))
            })
        })
        .collect::<Vec<Value>>();
    let (status, _) = create_temporal_entity(
        &http,
        &json!({
            "id": entity_id,
            "type": "Sensor",
            "reading": instances
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let rows = stored_attribute_history(&pool, &entity_id, &attribute_iri("reading")).await;
    assert_eq!(rows.len(), LATENESS_DAYS.len(), "one row per replayed observation");

    // Every row was written now, however old its observation is.
    for row in &rows {
        assert!(
            row.recorded_at > now - TimeDelta::hours(1),
            "a late observation is still ingested now, not backdated into an old chunk"
        );
        assert!(row.observed_at.is_some(), "each replayed observation declares its own instant");
    }

    // The declared instants span a year, and the rows still occupy one chunk,
    // which is what partitioning on the write clock buys.
    assert_eq!(
        occupied_chunks(&pool, &entity_id).await,
        1,
        "observations spanning a year still land in the single current ingestion chunk"
    );
    assert_eq!(
        attribute_history_chunk_interval(&pool).await,
        TimeDelta::days(7).num_microseconds().expect("7 days is representable in microseconds"),
        "the chunk interval the ingest assumption rests on is the declared 7 days"
    );

    // Each replayed observation is selectable on its own window and nothing else is.
    for days in LATENESS_DAYS {
        let observed = now - TimeDelta::days(days);
        let query = format!(
            "timerel=between&timeAt={}&endTimeAt={}",
            observation_instant(observed - TimeDelta::minutes(1)),
            observation_instant(observed + TimeDelta::minutes(1))
        );
        let (status, body) = get_temporal_entity(&http, &entity_id, &query).await;
        assert_eq!(status, StatusCode::OK);
        let matched = rendered_instances(&body, "reading");
        assert_eq!(matched.len(), 1, "the {days}-day-late window matches exactly its own observation");
        assert_eq!(matched[0]["value"], json!(days), "the {days}-day-late window returns its own value");
    }
    scope.finish().await.expect("cleanup failed");
}
