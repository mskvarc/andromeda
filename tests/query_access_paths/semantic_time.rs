//! Verification-matrix row "Semantic-time plans".
//!
//! Clause 5.2.21 admits four Temporal Properties and clause 4.11 makes
//! `observedAt` the default, so a window on one axis has to be answered from
//! that axis' own column: exactness first, and then the access path each axis
//! actually gets.
//!
//! The schema deliberately indexes one axis. `idx_attribute_history_stream_observed_at`
//! exists because `observedAt` is the default and carries the query volume;
//! migration 0048 records that a created, modified or deleted index is
//! benchmark-gated and none was accepted. These tests assert both halves of
//! that decision — the indexed axis reaches its index, the other three are
//! exact and stay on the stream-led path — so a later index is added because a
//! measurement asked for it rather than by accident.

use crate::{seeded_corpus::explain, seeded_history::seed};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use sqlx::PgPool;

/// One semantic time axis, and the column that answers for it.
struct Axis {
    /// The `timeproperty` value, for the failure message.
    timeproperty: &'static str,
    /// The `attribute_history` column holding it.
    column: &'static str,
}

/// The four axes clause 5.2.21 admits.
const AXES: &[Axis] = &[
    Axis {
        timeproperty: "observedAt",
        column: "observed_at",
    },
    Axis {
        timeproperty: "createdAt",
        column: "created_at",
    },
    Axis {
        timeproperty: "modifiedAt",
        column: "modified_at",
    },
    Axis {
        timeproperty: "deletedAt",
        column: "deleted_at",
    },
];

/// The instant every seeded axis is offset from.
const ORIGIN: &str = "TIMESTAMPTZ '2026-01-01 00:00:00Z'";

/// The observations one axis window selects across one stream, named by the
/// hour each observation sits at on the seeded grid.
async fn narrow_selection(pool: &PgPool, stream: uuid::Uuid, axis: &Axis, from: &str, to: &str) -> Vec<i32> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT (EXTRACT(EPOCH FROM (ah.observed_at - {ORIGIN})) / 3600)::int AS hour \
         FROM attribute_history AS ah \
         WHERE ah.stream_id = $1 AND ah.{} >= {from} AND ah.{} < {to} \
         ORDER BY hour",
        axis.column, axis.column
    )))
    .bind(stream)
    .fetch_all(pool)
    .await
    .unwrap_or_else(|error| unreachable!("the {} window must run: {error}", axis.timeproperty))
}

/// Clause 4.11: each axis answers from its own column, so one window selects a
/// different set of observations on each of the four. The seeded corpus offsets
/// `createdAt` a day back and `modifiedAt` a day forward from `observedAt`, and
/// only the tombstone of each stream carries a `deletedAt` at all, so the four
/// answers are decidable by arithmetic rather than by count alone.
#[tokio::test]
async fn every_axis_window_is_answered_from_its_own_column_over_a_narrow_selection() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, scope.id_prefix()).await;

    let from = format!("{ORIGIN} + INTERVAL '1 hour'");
    let to = format!("{ORIGIN} + INTERVAL '11 hours'");
    let mut selections = Vec::with_capacity(AXES.len());
    for axis in AXES {
        selections.push((axis.timeproperty, narrow_selection(pool, corpus.narrow_stream(), axis, &from, &to).await));
    }

    // Observation `g` sits at hour `g`, so the observed window selects hours 1
    // to 10. Its `createdAt` is a day earlier, so the same window selects the
    // observations whose hour is 24 greater — 25 to 34. Its `modifiedAt` is a
    // day later, which no seeded observation reaches inside the window, and its
    // `deletedAt` exists only on the tombstone at hour 1, two days after it.
    let expected: Vec<(&str, Vec<i32>)> = vec![
        ("observedAt", (1..=10).collect()),
        ("createdAt", (25..=34).collect()),
        ("modifiedAt", Vec::new()),
        ("deletedAt", Vec::new()),
    ];
    assert_eq!(selections, expected, "each axis must answer from its own column alone");

    scope.finish().await.expect("cleanup failed");
}

/// The same four windows over every stream of the entity, so the row's broad
/// selection is exercised beside its narrow one.
#[tokio::test]
async fn every_axis_window_stays_exact_over_a_broad_selection() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, scope.id_prefix()).await;

    for axis in AXES {
        let selected: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM attribute_history AS ah \
             JOIN temporal_attribute_streams AS s ON s.id = ah.stream_id \
             WHERE s.temporal_entity_key = $1 AND ah.{} IS NOT NULL",
            axis.column
        )))
        .bind(corpus.entity_key())
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| unreachable!("the broad {} selection must run: {error}", axis.timeproperty));

        let column_rows: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM attribute_history AS ah \
             JOIN temporal_attribute_streams AS s ON s.id = ah.stream_id \
             WHERE s.temporal_entity_key = $1 AND ah.{} IS NOT NULL",
            axis.column
        )))
        .bind(corpus.entity_key())
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| unreachable!("the broad {} population must run: {error}", axis.timeproperty));

        assert_eq!(
            selected, column_rows,
            "the broad {} selection must equal the rows that column populates",
            axis.timeproperty
        );
        assert!(selected > 0, "the corpus must populate {} for the row to mean anything", axis.timeproperty);
    }

    scope.finish().await.expect("cleanup failed");
}

/// The default axis carries the query volume, so it is the one the schema
/// indexes: a narrow observed window plans as an index scan on the stream-led
/// observed index rather than as a chunk scan.
#[tokio::test]
async fn a_narrow_observed_window_reaches_the_stream_led_observed_index() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, scope.id_prefix()).await;

    let statement = format!(
        "SELECT ah.value_number FROM attribute_history AS ah \
         WHERE ah.stream_id = '{}'::uuid \
           AND ah.observed_at >= {ORIGIN} + INTERVAL '1 hour' \
           AND ah.observed_at < {ORIGIN} + INTERVAL '11 hours'",
        corpus.narrow_stream()
    );
    let plan = explain(pool, &statement).await;
    assert!(
        plan.contains("idx_attribute_history_stream_observed_at"),
        "a narrow observed window must use the observed index: {plan}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Migration 0048 states that no created, modified or deleted index was
/// accepted, because none was measured to earn one. This records that decision
/// as an assertion rather than as prose: the day one is added, this test says
/// so, and the matrix row is re-measured instead of quietly gaining coverage.
#[tokio::test]
async fn no_axis_beside_the_default_carries_an_index_of_its_own() {
    let scope = TestEnvironment::from_suite("query-access-paths").await.expect("isolated test environment");
    let pool = scope.database_pool();

    let indexed_axes: Vec<String> = sqlx::query_scalar(
        "SELECT indexdef FROM pg_indexes \
         WHERE schemaname = 'public' AND tablename = 'attribute_history' \
           AND (indexdef LIKE '%created_at%' OR indexdef LIKE '%modified_at%' OR indexdef LIKE '%deleted_at%')",
    )
    .fetch_all(pool)
    .await
    .expect("the index catalogue must be readable");

    assert!(
        indexed_axes.is_empty(),
        "an axis beside observedAt gained an index; re-measure the semantic-time row before keeping it: {indexed_axes:?}"
    );

    scope.finish().await.expect("cleanup failed");
}
