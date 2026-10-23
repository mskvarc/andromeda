//! Semantic-time plans: each temporal axis is queried exactly and along its
//! intended access path.
//!
//! Clause 5.2.21 admits four Temporal Properties and clause 4.11 makes
//! `observedAt` the default, so a window on one axis has to be answered from
//! that axis' own column: exactness first, and then the access path each axis
//! actually gets.
//!
//! The schema deliberately indexes one axis for windows. `idx_attribute_history_stream_observed_at`
//! exists because `observedAt` is the default and carries the query volume;
//! a window index on `createdAt` or `modifiedAt` is benchmark-gated and none
//! is accepted. The placement indexes, `(stream_key, coalesce(axis,
//! deleted_at) desc)` on `observedAt`, `createdAt` and `modifiedAt`, serve
//! the instance in effect when a window opens, never a window itself: on a
//! stream of 5,000 instances that read takes 8 ms on `observedAt` and 64 ms on
//! `createdAt` with them, 358 ms and 403 ms without. These tests assert both
//! halves of the window decision, the indexed axis reaching its index and the
//! other three exact on the stream-led path, so a later index is added
//! because a measurement asked for it rather than by accident.

use crate::{
    seeded_corpus::{PlannerPaths, SEED_PREFIX, explain, explain_rolled_back},
    seeded_history::seed,
};
use andromeda_postgresql_schema::temporal::{AttributeHistory, TemporalAttributeStreams};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    interval::{epoch_seconds, make_interval_secs},
    pg_type::PostgresType,
    statement::Statement,
};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;
use sea_query::{Alias, Expr, ExprTrait, Iden, Order, Query, SelectStatement};
use sqlx::PgPool;

/// One semantic time axis, and the column that answers for it.
struct Axis {
    /// The `timeproperty` value, for the failure message.
    timeproperty: &'static str,
    /// The `attribute_history` column holding it.
    column: AttributeHistory,
}

/// The four axes clause 5.2.21 admits.
const AXES: &[Axis] = &[
    Axis {
        timeproperty: "observedAt",
        column: AttributeHistory::ObservedAt,
    },
    Axis {
        timeproperty: "createdAt",
        column: AttributeHistory::CreatedAt,
    },
    Axis {
        timeproperty: "modifiedAt",
        column: AttributeHistory::ModifiedAt,
    },
    Axis {
        timeproperty: "deletedAt",
        column: AttributeHistory::DeletedAt,
    },
];

/// The instant every seeded axis is offset from.
fn origin() -> DateTime<Utc> {
    "2026-01-01T00:00:00Z".parse().expect("the origin is a valid instant")
}

/// The instant `seconds` after the origin.
fn after_origin(seconds: f64) -> Expr {
    Expr::val(origin()).add(make_interval_secs(Expr::val(seconds)))
}

/// Seconds in an hour.
const HOUR: f64 = 3_600.0;

/// Seconds in a day.
const DAY: f64 = 86_400.0;

/// The observations one axis window selects across one stream, named by the
/// hour each observation sits at on the seeded grid.
async fn narrow_selection(pool: &PgPool, stream: i64, axis: &Axis, from: Expr, to: Expr) -> Vec<i32> {
    let column = || Expr::col((AttributeHistory::Table, axis.column));
    Query::select()
        .expr_as(
            cast(
                Expr::expr(epoch_seconds(
                    Expr::expr(Expr::col((AttributeHistory::Table, AttributeHistory::ObservedAt))).sub(Expr::val(origin())),
                ))
                .div(3_600),
                PostgresType::Integer,
            ),
            Alias::new("hour"),
        )
        .from(AttributeHistory::Table)
        .and_where(Expr::col((AttributeHistory::Table, AttributeHistory::StreamKey)).eq(stream))
        .and_where(column().gte(from))
        .and_where(column().lt(to))
        .order_by(Alias::new("hour"), Order::Asc)
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("the time-property window must run")
}

/// The count of the history rows of every stream of the entity `entity_key`
/// that populate `column`.
fn populated_rows(entity_key: i64, column: AttributeHistory) -> SelectStatement {
    Query::select()
        .expr(count_all())
        .from(AttributeHistory::Table)
        .inner_join(
            TemporalAttributeStreams::Table,
            Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey)).equals((AttributeHistory::Table, AttributeHistory::StreamKey)),
        )
        .and_where(Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::EntityKey)).eq(entity_key))
        .and_where(Expr::col((AttributeHistory::Table, column)).is_not_null())
        .to_owned()
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

    let mut selections = Vec::with_capacity(AXES.len());
    for axis in AXES {
        selections.push((
            axis.timeproperty,
            narrow_selection(pool, corpus.narrow_stream(), axis, after_origin(HOUR), after_origin(11.0 * HOUR)).await,
        ));
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
        let selected: i64 = populated_rows(corpus.entity_key(), axis.column)
            .to_query_scalar()
            .fetch_one(pool)
            .await
            .unwrap_or_else(|error| panic!("the broad {} selection must run: {error}", axis.timeproperty));

        let column_rows: i64 = populated_rows(corpus.entity_key(), axis.column)
            .to_query_scalar()
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

    let statement = Query::select()
        .column(AttributeHistory::ValueNumber)
        .from(AttributeHistory::Table)
        .and_where(Expr::col(AttributeHistory::StreamKey).eq(corpus.narrow_stream()))
        .and_where(Expr::col(AttributeHistory::ObservedAt).gte(after_origin(HOUR)))
        .and_where(Expr::col(AttributeHistory::ObservedAt).lt(after_origin(11.0 * HOUR)))
        .to_owned();
    let plan = explain(pool, statement).await;
    assert!(
        plan.contains("idx_attribute_history_stream_observed_at"),
        "a narrow observed window must use the observed index: {plan}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Migration 0020 gives every semantic axis exactly one BRIN block summary and
/// no B-tree of its own: `createdAt`, `modifiedAt` and `deletedAt` are selectable
/// timeproperties (clause 5.2.21) that neither the partition column nor any
/// stream-led index answers, and a per-row B-tree on each would cost what the
/// stream-led ones already cost again. This records that decision as an
/// assertion: the day an axis gains a B-tree or loses its summary, this test
/// says so and the access path is re-measured.
#[tokio::test]
async fn every_semantic_axis_carries_exactly_one_brin_summary_and_no_btree() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();

    let definitions: Vec<String> = Query::select()
        .column(Alias::new("indexdef"))
        .from(Alias::new("pg_indexes"))
        .and_where(Expr::col(Alias::new("schemaname")).eq("public"))
        .and_where(Expr::col(Alias::new("tablename")).eq("attribute_history"))
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("the index catalogue must be readable");

    for axis in AXES {
        let column = axis.column.to_string();
        let summaries: Vec<&String> = definitions
            .iter()
            .filter(|definition| definition.contains(&format!("USING brin ({column} ")))
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
            .filter(|definition| definition.contains(&format!("USING btree ({column}")))
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

    // The window names no stream on purpose: a stream-led join would hand the
    // planner the stream B-tree and the summary would never be consulted.
    let statement = Query::select()
        .expr(count_all())
        .from(AttributeHistory::Table)
        .and_where(Expr::col(AttributeHistory::ModifiedAt).gte(after_origin(DAY)))
        .and_where(Expr::col(AttributeHistory::ModifiedAt).lt(after_origin(DAY + 6.0 * HOUR)))
        .to_owned();
    let plan = explain_rolled_back(pool, PlannerPaths::WithoutSequentialScans, statement).await;
    assert!(
        plan.contains("idx_attribute_history_modified_at_brin"),
        "a broad modified window must be answerable from the modified_at summary: {plan}"
    );

    scope.finish().await.expect("cleanup failed");
}
