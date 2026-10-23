//! Temporal `q` plans: the instances a Query Term is checked against are read
//! along stream-led index ranges, whatever the length of the history.
//!
//! Clause 5.7.4.4 step S2 checks `q` against the Attribute instances valid
//! within the temporal window, and clause 4.11 with Annex C clause C.5.5 counts
//! among them the instance in effect when an interval with a start opens. The
//! stage-1 statement reads the instances inside the window as one range of
//! `idx_attribute_history_stream_observed_at` per stream, and the instance in
//! effect with one descending read per stream of
//! `idx_attribute_history_stream_observed_placement`, stopped after the rows
//! tied with its first. The statement is the one the broker builds, rendered
//! with its values inline, as a custom plan.

use crate::{
    seeded_corpus::{SEED_PREFIX, explain},
    seeded_history::{HISTORY_ATTRIBUTE, WINDOWED_ENTITIES, WindowedHistory, remove_windowed_history, seed_window_and_prior},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, temporal_format::TemporalFormat, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity_selection::EntitySelection,
    filter::parser,
    page_window::PageWindow,
    temporal::{
        compiled::CompiledTemporalQuery,
        instance_selection::InstanceSelection,
        representation::Representation,
        window::{TemporalWindow, TimeInterval},
    },
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    history_source::HistoryBranch,
};
use andromeda_postgresql_seaquery::{
    explain::{Explain, ExplainOption},
    statement::Statement,
};
use andromeda_postgresql_temporal_query::stage1::{Stage1Query, build_stage1_entity_ids};
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use chrono::{DateTime, TimeDelta, Utc};
use pretty_assertions::assert_eq;
use regex::Regex;
use sea_query::SelectStatement;
use sqlx::PgPool;

/// Observations per stream of the shorter corpus.
const SHORT_HISTORY: i64 = 200;

/// Observations per stream of the longer corpus, four times the shorter.
const LONG_HISTORY: i64 = 800;

/// The instant `minute` minutes after the seeded origin.
fn minute(minute: i64) -> DateTime<Utc> {
    "2026-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("a valid instant").get() + TimeDelta::minutes(minute)
}

/// The stage-1 statements of `q=<history>==<value in effect>` over the
/// corpus' window, with a page wide enough for every seeded Entity.
fn stage1(corpus: &WindowedHistory) -> Stage1Query {
    let mut compiled = CompiledTemporalQuery::builder()
        .entities(EntitySelection::builder().build())
        .window(TemporalWindow::builder().timeproperty(TimeProp::Observed).build())
        .instances(InstanceSelection::builder().build())
        .representation(
            Representation::builder()
                .format(TemporalFormat::Normalized)
                .sys_attrs(SysAttrsInclusion::Strip)
                .build(),
        )
        .build();
    compiled.window.interval = TimeInterval::Between {
        from: minute(corpus.window_start_minute()),
        to: minute(corpus.window_end_minute()),
    };
    compiled.entities.q_expr = Some(parser::parse(&format!("\"{HISTORY_ATTRIBUTE}\"=={}", corpus.value_in_effect())).expect("a valid q"));
    let names = QueryNames {
        attributes: ResolvedNames::from_pairs([(
            HISTORY_ATTRIBUTE.parse::<AttributeName>().expect("a valid attribute name"),
            AttributeNameKey::from(corpus.attribute()),
        )]),
        types: ResolvedNames::default(),
        ..QueryNames::default()
    };
    build_stage1_entity_ids(&HistoryBranch::live(), &compiled, &PageWindow::offset(1000, 0), CountPolicy::Emit, &names).expect("a renderable stage-1 query")
}

/// The stage-1 page statement.
fn page_statement(corpus: &WindowedHistory) -> SelectStatement {
    stage1(corpus).select
}

/// Clause 4.11 with Annex C clause C.5.5: no observation inside the window
/// carries the queried value, so every Entity is selected by the value its
/// stream held when the window opened -- except the one whose value was
/// deleted before it opened, which holds nothing then (clause 4.5.7).
async fn assert_selects_every_entity_but_the_tombstoned(pool: &PgPool, corpus: &WindowedHistory) {
    let selected: Vec<String> = page_statement(corpus)
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("the stage-1 page must run");
    let expected = usize::try_from(WINDOWED_ENTITIES - 1).expect("a small count");
    assert_eq!(selected.len(), expected, "{selected:?}");
    assert!(!selected.iter().any(|id| id == corpus.tombstoned()), "{selected:?}");
}

/// The generic plan of a statement whose values are left as placeholders:
/// the plan a prepared statement settles on once PostgreSQL stops planning it
/// for each execution's values.
async fn explain_generic(pool: &PgPool, statement: SelectStatement) -> String {
    let lines: Vec<String> = Explain::new(statement)
        .with(ExplainOption::GenericPlan)
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("the generic plan must be readable");
    lines.join("\n")
}

/// One node of a text `EXPLAIN`: its own line and the detail lines under it.
struct PlanNode<'a> {
    /// How far the node is indented, which orders it among its neighbours.
    depth: usize,
    /// The node's own line.
    line: &'a str,
    /// The `Index Cond`, `Filter` and other lines describing the node.
    details: Vec<&'a str>,
}

/// The nodes of a text `EXPLAIN`, in plan order.
fn plan_nodes(plan: &str) -> Vec<PlanNode<'_>> {
    let mut nodes: Vec<PlanNode<'_>> = Vec::new();
    for line in plan.lines() {
        match (line.find("->"), nodes.last_mut()) {
            (Some(depth), _) => nodes.push(PlanNode {
                depth,
                line,
                details: Vec::new(),
            }),
            (None, None) => nodes.push(PlanNode {
                depth: 0,
                line,
                details: Vec::new(),
            }),
            (None, Some(node)) => node.details.push(line.trim()),
        }
    }
    nodes
}

/// Whether a node scans `index` under an alias of the `alias` relation; a
/// hypertable names its chunk indexes after the parent index and suffixes the
/// alias with the chunk.
fn scans(node: &PlanNode<'_>, index: &str, alias: &str) -> bool {
    node.line.contains("Scan")
        && node.line.contains(index)
        && (node.line.ends_with(&format!(" {alias}")) || node.line.contains(&format!(" {alias}_")) || node.line.contains(&format!(" {alias} ")))
}

/// Whether some `Limit` node reads `index` under `alias` somewhere beneath it:
/// a descending probe that stops at the first entry.
fn limited_scan(plan: &str, index: &str, alias: &str) -> bool {
    let nodes = plan_nodes(plan);
    nodes.iter().enumerate().any(|(position, node)| {
        node.line.contains("Limit")
            && nodes
                .iter()
                .skip(position + 1)
                .take_while(|below| below.depth > node.depth)
                .any(|below| scans(below, index, alias))
    })
}

/// The shared buffers the whole statement touched, hit or read.
fn buffers_touched(plan: &str) -> u64 {
    let pattern = Regex::new(r"Buffers: shared hit=(\d+)(?: read=(\d+))?").expect("a valid pattern");
    let captures = pattern.captures(plan).expect("the plan reports its buffers");
    captures
        .iter()
        .skip(1)
        .flatten()
        .map(|count| count.as_str().parse::<u64>().expect("a buffer count"))
        .sum()
}

/// The instances inside the window are read as one range bounded on both
/// sides by the window, and no scan of the history is bounded by the start
/// alone without naming its stream -- the range the disjunctive probe read
/// once per entity, over every stream of the tenant. Nothing aggregates the
/// history either. On a corpus this small the planner may hash the window
/// probe instead of probing it per entity; either way it reads the window, not
/// the history before it.
#[tokio::test]
async fn temporal_plans_the_window_probe_is_a_bounded_range() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed_window_and_prior(pool, SEED_PREFIX, SHORT_HISTORY).await;
    assert_selects_every_entity_but_the_tombstoned(pool, &corpus).await;

    let plan = explain(pool, page_statement(&corpus)).await;
    let nodes = plan_nodes(&plan);
    let bounded = nodes.iter().any(|node| {
        node.line.contains(" on ")
            && node.line.contains(" qh")
            && node.details.iter().any(|detail| {
                (detail.starts_with("Index Cond:") || detail.starts_with("Recheck Cond:"))
                    && detail.contains("observed_at >= ")
                    && detail.contains("observed_at < ")
            })
    });
    assert!(bounded, "the window probe must read a range bounded by the window: {plan}");
    let unled = nodes.iter().flat_map(|node| &node.details).any(|detail| {
        detail.starts_with("Index Cond:") && detail.contains("observed_at <") && !detail.contains("observed_at >=") && !detail.contains("stream_key")
    });
    assert!(!unled, "no scan may range over the history before the start without its stream: {plan}");
    assert!(!plan.contains("BitmapOr"), "{plan}");
    assert!(!plan.contains("Aggregate"), "{plan}");

    scope.finish().await.expect("cleanup failed");
}

/// The latest row placed at or before the start, an observation or a
/// deletion with no `observedAt`, is one limited descending read per stream
/// on the placement index.
#[tokio::test]
async fn temporal_plans_the_instance_in_effect_is_one_limited_read_per_stream_on_the_placement_index() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed_window_and_prior(pool, SEED_PREFIX, SHORT_HISTORY).await;

    let plan = explain(pool, page_statement(&corpus)).await;
    assert!(
        limited_scan(&plan, "idx_attribute_history_stream_observed_placement", "rh"),
        "the instance in effect must be one limited read on the placement index: {plan}"
    );
    assert!(!plan.contains("Sort Key: (COALESCE"), "the placement order must come from the index: {plan}");

    scope.finish().await.expect("cleanup failed");
}

/// The broker runs the statement prepared, and PostgreSQL plans a prepared
/// statement generically once it has run a few times, without the bound
/// instants. The in-effect read must still be one limited read on the
/// placement index rather than a block summary read as a lossy bitmap once
/// per stream.
#[tokio::test]
async fn temporal_plans_the_generic_plan_keeps_the_in_effect_read_on_the_placement_index() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let corpus = seed_window_and_prior(pool, SEED_PREFIX, SHORT_HISTORY).await;

    let plan = explain_generic(pool, stage1(&corpus).select).await;
    assert!(
        limited_scan(&plan, "idx_attribute_history_stream_observed_placement", "rh"),
        "the instance in effect must stay one limited read on the placement index: {plan}"
    );
    assert!(!plan.contains("observed_at_brin") && !plan.contains("deleted_at_brin"), "{plan}");

    scope.finish().await.expect("cleanup failed");
}

/// The probe's cost does not depend on history length: with four times the history
/// before the window, the statement touches barely more buffers, where a
/// per-instance aggregate over the stream grows with the square of it.
#[tokio::test]
async fn temporal_plans_the_q_probe_stays_bounded_as_the_history_grows() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();

    let short = seed_window_and_prior(pool, SEED_PREFIX, SHORT_HISTORY).await;
    assert_selects_every_entity_but_the_tombstoned(pool, &short).await;
    let short_buffers = buffers_touched(&explain(pool, page_statement(&short)).await);

    remove_windowed_history(pool, SEED_PREFIX).await;

    let long = seed_window_and_prior(pool, SEED_PREFIX, LONG_HISTORY).await;
    assert_selects_every_entity_but_the_tombstoned(pool, &long).await;
    let long_plan = explain(pool, page_statement(&long)).await;
    let long_buffers = buffers_touched(&long_plan);

    assert!(
        long_buffers * 2 < short_buffers * 3,
        "four times the history must touch under 1.5 times the buffers: {short_buffers} then {long_buffers}: {long_plan}"
    );

    scope.finish().await.expect("cleanup failed");
}
