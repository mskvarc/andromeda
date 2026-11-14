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
        condition = condition.add(is_any_of(SeaExpr::col((alias.clone(), HistoryGrain::AttributeNameKey)), keys));
    }

    // Temporal filters (timerel + time_at), on the requested axis alone.
    condition.add(axis_window(Ah, compiled.window.timeproperty, &compiled.window.interval))
}

#[cfg(test)]
mod tests {
    use crate::stage1::{
        build_stage1_entity_ids,
        query_fixture::{built, instant, iri, minimal_compiled, speed_and_address, typed, vehicle_is_known, vehicle_selector},
    };
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_window::PageWindow, temporal::window::TimeInterval};
    use andromeda_postgresql_query::{dictionary::query_names::QueryNames, history_source::HistoryBranch};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Alias, IntoIden, SelectStatement};

    /// The stream catalogue carries the grain, so the correlation is on the
    /// surrogate entity key rather than on a repeated entity URI.
    #[test]
    fn the_stream_probe_reads_the_streams_of_the_selected_entity() {
        let compiled = minimal_compiled();
        let statement = built(&compiled, &QueryNames::default());
        assert_statement_snapshot!("window-match-the-stream-probe-reads-the-streams-of-the-selected-entity", statement);
    }

    /// Clause 4.11 defaults the selector to `observedAt` and clause 5.2.21
    /// admits only the four semantic axes, so the window filters the requested
    /// column and nothing stands in for a missing value on it: the bound on
    /// the column keeps out a row that carries no instant on the axis.
    #[test]
    fn each_time_property_filters_its_own_column_with_no_fallback() {
        let before_on = |prop: TimeProp| {
            let mut compiled = minimal_compiled();
            compiled.window.timeproperty = prop;
            compiled.window.interval = TimeInterval::Before(instant("2030-01-01T00:00:00Z"));
            built(&compiled, &QueryNames::default())
        };
        let observed = before_on(TimeProp::Observed);
        assert_statement_snapshot!("window-match-each-time-property-filters-its-own-column-with-no-fallback-1", observed);
        let created = before_on(TimeProp::Created);
        assert_statement_snapshot!("window-match-each-time-property-filters-its-own-column-with-no-fallback-2", created);
        let modified = before_on(TimeProp::Modified);
        assert_statement_snapshot!("window-match-each-time-property-filters-its-own-column-with-no-fallback-3", modified);
        let deleted = before_on(TimeProp::Deleted);
        assert_statement_snapshot!("window-match-each-time-property-filters-its-own-column-with-no-fallback-4", deleted);
    }

    /// A bounded window is the only reason to read the hypertable, so an
    /// unbounded one stays on the catalogue.
    #[test]
    fn the_history_hypertable_is_joined_only_for_a_bounded_window() {
        let compiled = minimal_compiled();
        let unbounded = built(&compiled, &QueryNames::default());
        assert_statement_snapshot!("window-match-the-history-hypertable-is-joined-only-for-a-bounded-window-1", unbounded);

        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let bounded = built(&compiled, &QueryNames::default());
        assert_statement_snapshot!("window-match-the-history-hypertable-is-joined-only-for-a-bounded-window-2", bounded);
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
        let statement = built(&compiled, &speed_and_address());
        assert_statement_snapshot!("window-match-the-attrs-selection-narrows-the-source-by-dictionary-key", statement);
    }

    /// An attribute the dictionary never keyed was never recorded, so a
    /// query naming only such attributes matches no Entity.
    #[test]
    fn an_attribute_never_keyed_matches_nothing() {
        let mut compiled = minimal_compiled();
        compiled.instances.attr_filter = vec![iri("https://example.com/unknown")];
        let statement = built(&compiled, &speed_and_address());
        assert_statement_snapshot!("window-match-an-attribute-never-keyed-matches-nothing", statement);
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
        let page = stage1.select;
        assert_statement_snapshot!("window-match-the-page-window-match-keeps-comparing-the-identifier-1", page);
        let count = stage1.count.expect("count requested");
        assert_statement_snapshot!("window-match-the-page-window-match-keeps-comparing-the-identifier-2", count);
    }

    /// Clause 5.7.4: on the live branch an entity is selected by its queued
    /// entries as by its recorded rows, so stage 1 reads both.
    #[test]
    fn a_live_stage_one_reads_the_pending_entries_beside_the_history() {
        let mut compiled = minimal_compiled();
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        let statement = built(&compiled, &QueryNames::default());
        assert_statement_snapshot!("window-match-a-live-stage-one-reads-the-pending-entries-beside-the-history", statement);
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
        let page = stage1.select;
        assert_statement_snapshot!("window-match-a-snapshot-stage-one-reads-no-pending-entry-1", page);
        let count = stage1.count.expect("count requested");
        assert_statement_snapshot!("window-match-a-snapshot-stage-one-reads-no-pending-entry-2", count);
    }

    /// The page and the count of a stage 1 over `branch` for a window opening
    /// at `opening`, or never closing when `opening` is `None`.
    fn page_and_count(branch: &HistoryBranch, opening: Option<&str>) -> [SelectStatement; 2] {
        let mut compiled = minimal_compiled();
        if let Some(opening) = opening {
            compiled.window.interval = TimeInterval::After(instant(opening));
        }
        let stage1 =
            build_stage1_entity_ids(branch, &compiled, &PageWindow::offset(20, 0), CountPolicy::Emit, &QueryNames::default()).expect("valid temporal query");
        [stage1.select, stage1.count.expect("count requested")]
    }

    /// The page's matches carry the identifier it walks the Evolutions in;
    /// the count reads every match, by the key the recorded streams carry,
    /// and reads no catalogue row of the Evolutions it holds.
    #[test]
    fn a_window_match_over_an_empty_queue_compares_the_page_by_identifier_and_the_count_by_key() {
        let [recorded_page, recorded_count] = page_and_count(&HistoryBranch::live_recorded(), Some("2024-01-01T00:00:00Z"));
        assert_statement_snapshot!(
            "window-match-a-window-match-over-an-empty-queue-compares-the-page-by-identifier-and-the-count-by-key-1",
            recorded_page
        );
        assert_statement_snapshot!(
            "window-match-a-window-match-over-an-empty-queue-compares-the-page-by-identifier-and-the-count-by-key-2",
            recorded_count
        );
        let snapshot = HistoryBranch::recorded_in(Alias::new("snap_7").into_iden());
        let [snapshot_page, snapshot_count] = page_and_count(&snapshot, Some("2024-01-01T00:00:00Z"));
        assert_statement_snapshot!(
            "window-match-a-window-match-over-an-empty-queue-compares-the-page-by-identifier-and-the-count-by-key-3",
            snapshot_page
        );
        assert_statement_snapshot!(
            "window-match-a-window-match-over-an-empty-queue-compares-the-page-by-identifier-and-the-count-by-key-4",
            snapshot_count
        );
    }

    #[test]
    fn a_stream_match_over_an_empty_queue_compares_the_page_by_identifier_and_the_count_by_key() {
        let [page, count] = page_and_count(&HistoryBranch::live_recorded(), None);
        assert_statement_snapshot!(
            "window-match-a-stream-match-over-an-empty-queue-compares-the-page-by-identifier-and-the-count-by-key-1",
            page
        );
        assert_statement_snapshot!(
            "window-match-a-stream-match-over-an-empty-queue-compares-the-page-by-identifier-and-the-count-by-key-2",
            count
        );
    }

    /// PostgreSQL pushes a restriction into the branches of a union, never a
    /// join clause, so a source reading the queue names the Evolution in both.
    #[test]
    fn a_window_match_reading_the_queue_takes_the_evolution_inside_both_branches() {
        let [page, count] = page_and_count(&HistoryBranch::live(), Some("2024-01-01T00:00:00Z"));
        assert_statement_snapshot!("window-match-a-window-match-reading-the-queue-takes-the-evolution-inside-both-branches-1", page);
        assert_statement_snapshot!(
            "window-match-a-window-match-reading-the-queue-takes-the-evolution-inside-both-branches-2",
            count
        );
    }
}
