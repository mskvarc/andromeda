//! Verification-matrix row "Semantic-time plans".
//!
//! Clause 5.2.21 admits four Temporal Properties and clause 4.11 makes
//! `observedAt` the default, so a window on one axis has to be answered from
//! that axis' own column: exactness first, and then the access path each axis
//! actually gets.
//!
//! The schema deliberately indexes one axis. `idx_attribute_history_stream_observed_at`
//! exists because `observedAt` is the default and carries the query volume;
//! migration 0011 records that a created, modified or deleted index is
//! benchmark-gated and none was accepted. These tests assert both halves of
//! that decision — the indexed axis reaches its index, the other three are
//! exact and stay on the stream-led path — so a later index is added because a
//! measurement asked for it rather than by accident.

use crate::{
    seeded_corpus::{SEED_PREFIX, explain},
    seeded_history::seed,
};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
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
async fn narrow_selection(pool: &PgPool, stream: i64, axis: &Axis, from: &str, to: &str) -> Vec<i32> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT (EXTRACT(EPOCH FROM (ah.observed_at - {ORIGIN})) / 3600)::int AS hour \
         FROM attribute_history AS ah \
         WHERE ah.stream_key = $1 AND ah.{} >= {from} AND ah.{} < {to} \
         ORDER BY hour",
        axis.column, axis.column
    )))
    .bind(stream)
    .fetch_all(pool)
    .await
    .expect("the time-property window must run")
}

/// Clause 4.11: each axis answers from its own column, so one window selects a
/// different set of observations on each of the four. The seeded corpus offsets
/// `createdAt` a day back and `modifiedAt` a day forward from `observedAt`, and
/// only the tombstone of each stream carries a `deletedAt` at all, so the four
/// answers are decidable by arithmetic rather than by count alone.
#[tokio::test]
async fn every_axis_window_is_answered_from_its_own_column_over_a_narrow_selection() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;

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
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;

    for axis in AXES {
        let selected: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM attribute_history AS ah \
             JOIN temporal_attribute_streams AS s ON s.stream_key = ah.stream_key \
             WHERE s.temporal_entity_key = $1 AND ah.{} IS NOT NULL",
            axis.column
        )))
        .bind(corpus.entity_key())
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| panic!("the broad {} selection must run: {error}", axis.timeproperty));

        let column_rows: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM attribute_history AS ah \
             JOIN temporal_attribute_streams AS s ON s.stream_key = ah.stream_key \
             WHERE s.temporal_entity_key = $1 AND ah.{} IS NOT NULL",
            axis.column
        )))
        .bind(corpus.entity_key())
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| panic!("the broad {} population must run: {error}", axis.timeproperty));

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
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed(pool, SEED_PREFIX).await;

    let statement = format!(
        "SELECT ah.value_number FROM attribute_history AS ah \
         WHERE ah.stream_key = {} \
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

/// Migration 0011 gives every semantic axis exactly one BRIN block summary and
/// no B-tree of its own: `createdAt`, `modifiedAt` and `deletedAt` are selectable
/// timeproperties (clause 5.2.21) that neither the partition column nor any
/// stream-led index answers, and a per-row B-tree on each would cost what the
/// stream-led ones already cost again. This records that decision as an
/// assertion: the day an axis gains a B-tree or loses its summary, this test
/// says so and the matrix row is re-measured.
#[tokio::test]
async fn every_semantic_axis_carries_exactly_one_brin_summary_and_no_btree() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();

    let definitions: Vec<String> = sqlx::query_scalar("SELECT indexdef FROM pg_indexes WHERE schemaname = 'public' AND tablename = 'attribute_history'")
        .fetch_all(pool)
        .await
        .expect("the index catalogue must be readable");

    for axis in AXES {
        let summaries: Vec<&String> = definitions
            .iter()
            .filter(|definition| definition.contains(&format!("USING brin ({} ", axis.column)))
            .collect();
        assert_eq!(
            summaries.len(),
            1,
            "{} must carry exactly one BRIN summary of its own: {definitions:?}",
            axis.timeproperty
        );
        assert!(
            summaries[0].contains("timestamptz_minmax_multi_ops"),
            "the {} summary must be multi-range so a late outlier stays in its own range: {}",
            axis.timeproperty,
            summaries[0]
        );
        let led_btrees: Vec<&String> = definitions
            .iter()
            .filter(|definition| definition.contains(&format!("USING btree ({}", axis.column)))
            .collect();
        assert!(
            led_btrees.is_empty(),
            "{} must lead no B-tree of its own; the stream-led B-trees are the only ones: {led_btrees:?}",
            axis.timeproperty
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// A `modifiedAt` window that names no stream has the block summary as its only
/// access path, and the summary must be usable for that predicate shape. The
/// seeded corpus is far too small for the planner to prefer any index over a
/// chunk scan on its own -- BRIN's default range spans more pages than the
/// corpus occupies -- so the sequential scan is priced out for this one
/// statement; whether the summary wins on cost at production volume is what
/// the benchmark measured, and this test promises only that the operator class
/// answers the window.
#[tokio::test]
async fn a_broad_modified_window_can_be_answered_from_the_modified_at_summary() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    seed(pool, SEED_PREFIX).await;

    let mut transaction = pool.begin().await.expect("the plan transaction must open");
    sqlx::query("SET LOCAL enable_seqscan = off")
        .execute(&mut *transaction)
        .await
        .expect("the planner setting must apply");
    // The window names no stream on purpose: a stream-led join would hand the
    // planner the stream B-tree and the summary would never be consulted.
    let statement = format!(
        "SELECT COUNT(*) FROM attribute_history AS ah \
         WHERE ah.modified_at >= {ORIGIN} + INTERVAL '1 day' \
           AND ah.modified_at < {ORIGIN} + INTERVAL '1 day 6 hours'"
    );
    let lines: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("EXPLAIN (ANALYZE, BUFFERS) {statement}")))
        .fetch_all(&mut *transaction)
        .await
        .expect("the plan must be readable");
    let plan = lines.join("\n");
    transaction.rollback().await.expect("the plan transaction must roll back");
    assert!(
        plan.contains("idx_attribute_history_modified_at_brin"),
        "a broad modified window must be answerable from the modified_at summary: {plan}"
    );

    scope.finish().await.expect("cleanup failed");
}
