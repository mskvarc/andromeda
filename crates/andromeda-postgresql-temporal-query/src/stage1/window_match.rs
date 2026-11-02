//! The clause 5.7.4.4 step S1 match of stage 1 of a temporal query: the
//! correlated stream or history predicate that keeps an Evolution holding an
//! instance of a concerned attribute inside the window.

use crate::{
    alias::{Ah, S},
    stage1::evolution_walk::concerned_attribute_keys,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    history_source::{
        BesideComparison,
        EvolutionColumns,
        HistoryBranch,
        attribute_rows::{entity_attribute_rows, entity_stream_rows},
        axis::axis_window,
        grain::HistoryGrain,
    },
};
use andromeda_postgresql_schema::current::{E, Entities};
use andromeda_postgresql_seaquery::{literal::int_literal, pg_array::is_any_of};
use sea_query::{Condition, DynIden, Expr as SeaExpr, IntoIden, SelectStatement};

/// Build the correlated stream or history predicate shared by result and
/// count: a bounded window asks for an instance inside it, an unbounded one
/// only for a stream of a concerned attribute.
///
/// Over a source without queued entries the Evolution is compared beside the
/// source, so a window few Evolutions answer, or none, is found by a
/// semi-join over its rows instead of one probe per candidate. The page
/// compares the identifier, so its matches merge with the walk in identifier
/// order and the page stops at its last Evolution; the count, which reads
/// every match, compares the surrogate key, so a window most Evolutions
/// answer joins no catalogue row to its rows.
pub(super) fn temporal_match_subquery(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    names: &QueryNames,
    compared: BesideComparison,
) -> SelectStatement {
    let read = branch.entity_history(
        EvolutionColumns {
            id: SeaExpr::col((E, Entities::Id)),
            key: SeaExpr::col((E, Entities::EntityKey)),
        },
        compared,
    );
    let (source, alias) = if compiled.window.interval.is_bounded() {
        (entity_attribute_rows(branch, &read, Ah), Ah.into_iden())
    } else {
        (entity_stream_rows(branch, &read, S), S.into_iden())
    };
    let mut select = SelectStatement::new();
    select
        .expr(int_literal(1))
        .from(source)
        .cond_where(build_temporal_stage1_conditions(compiled, &alias, &names.attributes));
    if let Some(compared) = read.compared_beside(&alias) {
        select.and_where(compared);
    }
    select.take()
}

/// Build the clause 5.7.4.4 step S1 conditions of Stage 1 over the source
/// under `alias`: the concerned attributes and the window.
///
/// The attributes are compared by dictionary key on the source's own grain
/// column, which PostgreSQL pushes into every branch of the source, so the
/// queued entries of other attributes are dropped before the rules that
/// judge whether an entry is written; an attribute the dictionary never
/// keyed has no history and matches nothing.
fn build_temporal_stage1_conditions(compiled: &CompiledTemporalQuery, alias: &DynIden, attributes: &ResolvedNames<AttributeName>) -> Condition {
    let mut condition = Condition::all();

    if !compiled.instances.attr_filter.is_empty() {
        let keys: Vec<i32> = concerned_attribute_keys(compiled, attributes).into_iter().map(AttributeNameKey::get).collect();
        // clone: the alias is shared by every condition on the source.
        condition = condition.add(is_any_of(SeaExpr::col((alias.clone(), HistoryGrain::AttributeNameKey)), keys));
    }

    // Temporal filters (timerel + time_at), on the requested axis alone.
    condition.add(axis_window(Ah, compiled.window.timeproperty, &compiled.window.interval))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::stage1::{
        build_stage1_entity_ids,
        query_fixture::{built, instant, iri, minimal_compiled, speed_and_address, typed, vehicle_is_known, vehicle_selector},
    };
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_window::PageWindow, temporal::window::TimeInterval};
    use andromeda_postgresql_query::{dictionary::query_names::QueryNames, history_source::HistoryBranch};
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder, Value};

    /// The stream catalogue carries the grain, so the correlation is on the
    /// surrogate entity key rather than on a repeated entity URI.
    #[test]
    fn the_stream_probe_reads_the_streams_of_the_selected_entity() {
        let compiled = minimal_compiled();
        let (sql, _) = built(&compiled, &QueryNames::default());
        assert!(sql.contains("\"re\".\"id\" = \"e\".\"id\""), "{sql}");
        assert!(sql.contains("\"pe\".\"entity_id\" = \"e\".\"id\""), "{sql}");
    }

    /// Clause 4.11 defaults the selector to `observedAt` and clause 5.2.21
    /// admits only the four semantic axes, so the window filters the requested
    /// column and nothing stands in for a missing value on it: the bound on
    /// the column keeps out a row that carries no instant on the axis.
    #[test]
    fn each_time_property_filters_its_own_column_with_no_fallback() {
        let expected = [
            (TimeProp::Observed, r#""ah"."observed_at" <"#),
            (TimeProp::Created, r#""ah"."created_at" <"#),
            (TimeProp::Modified, r#""ah"."modified_at" <"#),
            (TimeProp::Deleted, r#""ah"."deleted_at" <"#),
        ];
        for (prop, column) in expected {
            let mut compiled = minimal_compiled();
            compiled.window.timeproperty = prop;
            compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));
            let (sql, _) = built(&compiled, &QueryNames::default());
            assert!(sql.contains(column), "{prop}: {sql}");
            assert!(!sql.contains("COALESCE"), "{prop}: {sql}");
            assert!(!sql.contains("\"ah\".\"recorded_at\""), "{prop}: {sql}");
        }
    }

    /// A bounded window is the only reason to read the hypertable, so an
    /// unbounded one stays on the catalogue.
    #[test]
    fn the_history_hypertable_is_joined_only_for_a_bounded_window() {
        let compiled = minimal_compiled();
        let (unbounded, _) = built(&compiled, &QueryNames::default());
        assert!(!unbounded.contains("attribute_history"), "{unbounded}");

        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let (bounded, _) = built(&compiled, &QueryNames::default());
        assert!(bounded.contains(r#") AS "ah" WHERE"#), "{bounded}");
    }

    /// The concerned attributes are compared by dictionary key on the
    /// source's own grain column, a restriction PostgreSQL pushes into each
    /// branch of the source, so the window match never reads the name
    /// dictionary.
    #[test]
    fn the_attrs_selection_narrows_the_source_by_dictionary_key() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        compiled.instances.attr_filter = vec![iri("https://example.com/speed")];
        let (sql, values) = built(&compiled, &speed_and_address());
        assert!(!sql.contains("attribute_names"), "{sql}");
        assert!(sql.contains(r#") AS "ah" WHERE "ah"."attribute_name_key" = $"#), "{sql}");
        assert!(values.0.iter().any(|value| matches!(value, Value::Int(Some(7)))), "{values:?}");
    }

    /// An attribute the dictionary never keyed was never recorded, so a
    /// query naming only such attributes matches no Entity.
    #[test]
    fn an_attribute_never_keyed_matches_nothing() {
        let mut compiled = minimal_compiled();
        compiled.instances.attr_filter = vec![iri("https://example.com/unknown")];
        let (sql, _) = built(&compiled, &speed_and_address());
        assert!(sql.contains(r#") AS "s" WHERE (FALSE"#), "{sql}");
    }

    /// Over recorded rows alone the page's window match compares the
    /// identifier beside its source, so its matches merge with the walk in
    /// identifier order; the count, which reads every match, compares the key.
    #[test]
    fn the_page_window_match_keeps_comparing_the_identifier() {
        let mut compiled = typed(vec![vehicle_selector()]);
        compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));
        let stage1 = build_stage1_entity_ids(
            &HistoryBranch::live_recorded(),
            &compiled,
            &PageWindow::offset(20, 0),
            CountPolicy::Emit,
            &vehicle_is_known(),
        )
        .expect("valid temporal query");
        let page = stage1.select.to_string(PostgresQueryBuilder);
        assert!(page.contains(r#") AS "ah" WHERE "ah"."observed_at" < "#), "{page}");
        assert!(page.contains(r#""ah"."entity_id" = "e"."id""#), "{page}");
        let count = stage1.count.expect("count requested").to_string(PostgresQueryBuilder);
        assert!(count.contains(r#""ah"."entity_key" = "e"."entity_key""#), "{count}");
        assert!(!count.contains(r#""ah"."entity_id" = "e"."id""#), "{count}");
    }

    /// Clause 5.7.4: on the live branch an entity is selected by its queued
    /// entries as by its recorded rows, so stage 1 reads both.
    #[test]
    fn a_live_stage_one_reads_the_pending_entries_beside_the_history() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let (sql, _) = built(&compiled, &QueryNames::default());
        let history = sql.split(r#") AS "ah" JOIN "attribute_names""#).next().expect("the history source");
        assert!(history.contains(r#"UNION ALL (SELECT "pe"."entity_id" AS "entity_id""#), "{sql}");
        assert!(history.contains(r#"FROM "temporal_append_entries" AS "pe""#), "{sql}");
    }

    /// A Snapshot branch records its history in the write's own transaction
    /// and has no queue, so its stage 1 reads the recorded relations alone.
    #[test]
    fn a_snapshot_stage_one_reads_no_pending_entry() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let snapshot = Alias::new("snap_7").into_iden();
        let stage1 = build_stage1_entity_ids(
            &HistoryBranch::recorded_in(snapshot),
            &compiled,
            &PageWindow::offset(20, 0),
            CountPolicy::Emit,
            &QueryNames::default(),
        )
        .expect("valid temporal query");
        for sql in [
            stage1.select.to_string(PostgresQueryBuilder),
            stage1.count.expect("count requested").to_string(PostgresQueryBuilder),
        ] {
            assert!(!sql.contains("temporal_append"), "{sql}");
            assert!(
                sql.contains(r#"FROM (SELECT * FROM "snap_7"."entities" WHERE "entities"."evolution_created_at" IS NOT NULL) AS "e""#),
                "{sql}"
            );
        }
    }

    /// The page and the count of a stage 1 over `branch` for a window opening
    /// at `opening`, or never closing when `opening` is `None`.
    fn page_and_count(branch: &HistoryBranch, opening: Option<&str>) -> [String; 2] {
        let mut compiled = minimal_compiled();
        if let Some(opening) = opening {
            compiled.window.interval = TimeInterval::After(instant(opening));
        }
        let stage1 =
            build_stage1_entity_ids(branch, &compiled, &PageWindow::offset(20, 0), CountPolicy::Emit, &QueryNames::default()).expect("valid temporal query");
        [
            stage1.select.to_string(PostgresQueryBuilder),
            stage1.count.expect("count requested").to_string(PostgresQueryBuilder),
        ]
    }

    /// The page's matches carry the identifier it walks the Evolutions in;
    /// the count reads every match, by the key the recorded streams carry,
    /// and reads no catalogue row of the Evolutions it holds.
    #[test]
    fn a_window_match_over_an_empty_queue_compares_the_page_by_identifier_and_the_count_by_key() {
        for branch in [HistoryBranch::live_recorded(), HistoryBranch::recorded_in(Alias::new("snap_7").into_iden())] {
            let [page, count] = page_and_count(&branch, Some("2024-01-01T00:00:00Z"));
            assert!(
                page.contains(r#") AS "ah" WHERE "ah"."observed_at" >= '2024-01-01 00:00:00.000000 +00:00' AND "ah"."entity_id" = "e"."id")"#),
                "{page}"
            );
            assert!(!page.contains(r#""re"."id" = "e"."id""#), "{page}");
            assert!(
                count.contains(r#") AS "ah" WHERE "ah"."observed_at" >= '2024-01-01 00:00:00.000000 +00:00' AND "ah"."entity_key" = "e"."entity_key")"#),
                "{count}"
            );
            assert!(!count.contains(r#"AS "re""#), "{count}");
        }
    }

    #[test]
    fn a_stream_match_over_an_empty_queue_compares_the_page_by_identifier_and_the_count_by_key() {
        let [page, count] = page_and_count(&HistoryBranch::live_recorded(), None);
        assert!(page.contains(r#") AS "s" WHERE TRUE AND "s"."entity_id" = "e"."id")"#), "{page}");
        assert!(count.contains(r#") AS "s" WHERE TRUE AND "s"."entity_key" = "e"."entity_key")"#), "{count}");
        assert!(!count.contains(r#"AS "re""#), "{count}");
    }

    /// PostgreSQL pushes a restriction into the branches of a union, never a
    /// join clause, so a source reading the queue names the Evolution in both.
    #[test]
    fn a_window_match_reading_the_queue_takes_the_evolution_inside_both_branches() {
        for sql in page_and_count(&HistoryBranch::live(), Some("2024-01-01T00:00:00Z")) {
            assert!(sql.contains(r#""re"."id" = "e"."id""#), "{sql}");
            assert!(sql.contains(r#""pe"."entity_id" = "e"."id""#), "{sql}");
            assert!(!sql.contains(r#""ah"."entity_id" = "e"."id""#), "{sql}");
        }
    }
}
