//! The candidate step a stage 1 statement takes: one bounded index read that
//! names the few Evolutions a filter can keep, so the page and the count are
//! cut from the walk narrowed to them rather than from every Evolution the
//! walk yields.
//!
//! A statement takes one step at most. Every filter the step does not answer
//! is tested on the narrowed walk, and a request no step applies to walks as
//! before. A `q` term gets no step of its own: history holds no index of its
//! values that a numeric write could afford to keep.

use crate::stage1::{
    entity_conditions::PatternTest,
    extent_candidates::{EXTENT_CANDIDATE_BOUND, EXTENT_SHORT_WALK, ExtentRead},
    pattern_candidates::pattern_candidates,
    scope_candidates::{SCOPE_CANDIDATE_BOUND, SCOPE_SHORT_WALK, scope_candidates},
    window_candidates::{WINDOW_CANDIDATE_BOUND, WINDOW_DENSE_WALK, WINDOW_SHORT_WALK, WindowRead},
};
use andromeda_ngsi_ld_query::{
    entity_selection::ConsideredEntities,
    pattern::Pattern,
    scope::query::ScopeQuery,
    temporal::{compiled::CompiledTemporalQuery, contributed_entities::ContributedEntities},
};
use andromeda_postgresql_query::{
    dictionary::query_names::QueryNames,
    error::RenderError,
    history_source::HistoryBranch,
    id_pattern_trigrams::{ID_PATTERN_CANDIDATE_BOUND, ID_PATTERN_SHORT_WALK, trigram_pattern},
    one_time_gate::COUNT_CANDIDATE_BOUND,
};
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use sea_query::SelectStatement;

/// The candidate step a stage 1 statement takes.
#[derive(Debug, Clone)]
pub(super) enum CandidateStep<'q> {
    /// The lone selector's `idPattern` (clause 5.7.2), when it leaves the
    /// identifier's start open: the catalogue's trigram index yields the
    /// identifiers it matches, each rechecked against it.
    IdPattern(&'q Pattern),
    /// The `scopeQ` (clause 4.19): the Scope path index yields the Evolutions
    /// with a recorded Scope matching one of its paths, beside the Entities a
    /// Context Source contributed to a split query, which the Scope query
    /// does not decide locally.
    ScopePaths(&'q ScopeQuery, &'q ContributedEntities),
    /// The geoquery (clause 4.10), when its relation admits a box: the stream
    /// extent index yields the Evolutions holding a stream of the
    /// `GeoProperty` whose extent meets it, beside the contributed Entities.
    GeometryExtents(ExtentRead<'q>),
    /// The window (clause 4.11) when it is bounded: the history rows inside
    /// it yield the Evolutions with an instance of a concerned attribute
    /// there.
    Window(WindowRead<'q>),
}

/// How the count of a stage 1 statement with a candidate step is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CountRead {
    /// From the walk narrowed to the candidates while they number at most the
    /// bound, else from the walk.
    Gated(u64),
    /// From the walk, whose window match already finds the Evolutions a
    /// window answers by a semi-join over the time indexes.
    Walked,
}

impl CountRead {
    /// How PostgreSQL may plan the count.
    pub(super) const fn planning(self) -> StatementPlanning {
        match self {
            // The count's arm is chosen by a bound only its values decide.
            CountRead::Gated(_) => StatementPlanning::BoundValues,
            CountRead::Walked => StatementPlanning::Cached,
        }
    }
}

impl<'q> CandidateStep<'q> {
    /// The step stage 1 takes for `compiled`, `None` when the walk answers
    /// alone.
    ///
    /// The cheapest bounded read that applies is taken: the identifier
    /// pattern, then the Scope paths, then the geometry extents, then the
    /// window. A requested ordering cuts the page in an order no candidates
    /// are read in, so it keeps the walk. The identifier pattern applies when one selector
    /// carries it: several selectors pair each pattern with its own type,
    /// which one read of the trigrams cannot.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::ReferenceGeometryEncoding`] when the geoquery's
    /// reference geometry has no EWKB spelling the store can bind.
    pub(super) fn chosen(compiled: &'q CompiledTemporalQuery, names: &QueryNames) -> Result<Option<CandidateStep<'q>>, RenderError> {
        match compiled.entities.considered {
            ConsideredEntities::Selected => {}
            ConsideredEntities::NoEntity => return Ok(None),
        }
        if compiled.ordering.is_some() {
            return Ok(None);
        }
        let pattern = match compiled.entities.selectors.as_slice() {
            [selector] => trigram_pattern(selector.identifier_axis()),
            _ => None,
        };
        if let Some(pattern) = pattern {
            return Ok(Some(CandidateStep::IdPattern(pattern)));
        }
        if let Some(scope_q) = compiled.entities.scope_q.as_ref() {
            return Ok(Some(CandidateStep::ScopePaths(scope_q, &compiled.contributed)));
        }
        if let Some(extents) = ExtentRead::of(compiled, names)? {
            return Ok(Some(CandidateStep::GeometryExtents(extents)));
        }
        Ok(WindowRead::of(compiled, names).map(CandidateStep::Window))
    }

    /// How many Evolutions the short walk visits before the candidates are
    /// read.
    pub(super) const fn short_walk(&self) -> u32 {
        match self {
            CandidateStep::IdPattern(_) => ID_PATTERN_SHORT_WALK,
            CandidateStep::ScopePaths(..) => SCOPE_SHORT_WALK,
            CandidateStep::GeometryExtents(_) => EXTENT_SHORT_WALK,
            CandidateStep::Window(_) => WINDOW_SHORT_WALK,
        }
    }

    /// The most candidates a page reads before it walks instead.
    pub(super) const fn bound(&self) -> u64 {
        match self {
            CandidateStep::IdPattern(_) => ID_PATTERN_CANDIDATE_BOUND,
            CandidateStep::ScopePaths(..) => SCOPE_CANDIDATE_BOUND,
            CandidateStep::GeometryExtents(_) => EXTENT_CANDIDATE_BOUND,
            CandidateStep::Window(_) => WINDOW_CANDIDATE_BOUND,
        }
    }

    /// The most Evolutions the full walk is trusted to visit in place of the
    /// candidates when the short walk's matches project the page full within
    /// them; `None` when the candidates are read whenever the short walk falls
    /// short.
    ///
    /// Only the window's read has a floor of its own: it reaches converted
    /// history through every batch summary, while the trigram, Scope path and
    /// extent reads cost what they find.
    pub(super) const fn dense_walk(&self) -> Option<u32> {
        match self {
            CandidateStep::IdPattern(_) | CandidateStep::ScopePaths(..) | CandidateStep::GeometryExtents(_) => None,
            CandidateStep::Window(_) => Some(WINDOW_DENSE_WALK),
        }
    }

    /// How PostgreSQL may plan the page.
    pub(super) const fn page_planning(&self) -> StatementPlanning {
        match self {
            // A gated-off branch still launches the workers of a Gather under
            // it, so the index reads run serially.
            CandidateStep::IdPattern(_) | CandidateStep::ScopePaths(..) | CandidateStep::GeometryExtents(_) => StatementPlanning::GatedBranches,
            // The window's read scans every batch summary of converted
            // history, which workers split.
            CandidateStep::Window(_) => StatementPlanning::BoundValues,
        }
    }

    /// How the count is read.
    pub(super) const fn count_read(&self) -> CountRead {
        match self {
            CandidateStep::IdPattern(_) | CandidateStep::ScopePaths(..) | CandidateStep::GeometryExtents(_) => CountRead::Gated(COUNT_CANDIDATE_BOUND),
            CandidateStep::Window(_) => CountRead::Walked,
        }
    }

    /// Whether the narrowed walk still tests the selector's pattern.
    pub(super) const fn pattern_test(&self) -> PatternTest {
        match self {
            CandidateStep::IdPattern(_) => PatternTest::Answered,
            CandidateStep::ScopePaths(..) | CandidateStep::GeometryExtents(_) | CandidateStep::Window(_) => PatternTest::Tested,
        }
    }

    /// The candidates of `branch`, unbounded: surrogate keys over recorded
    /// rows alone, identifiers where the branch reads its queue.
    pub(super) fn candidates(&self, branch: &HistoryBranch) -> SelectStatement {
        match self {
            CandidateStep::IdPattern(pattern) => pattern_candidates(branch, pattern),
            CandidateStep::ScopePaths(scope_q, contributed) => scope_candidates(branch, scope_q, contributed),
            CandidateStep::GeometryExtents(extents) => extents.candidates(branch),
            CandidateStep::Window(window) => window.candidates(branch),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::stage1::{
        build_stage1_entity_ids,
        query_fixture::{PageBranches, id_pattern, instant, near_query, recorded_stage1, sensor_names, sensors},
    };
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        filter::parser,
        page_window::PageWindow,
        scope::query::ScopeQuery,
        temporal::window::TimeInterval,
    };
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    /// Clause 5.7.4.4 refuses a query without a temporal query, so a `q`
    /// query always carries a window, and the window's step is the one it
    /// takes.
    #[test]
    fn a_q_filter_takes_no_step_of_its_own() {
        let mut compiled = sensors();
        compiled.entities.q_expr = Some(parser::parse("\"https://uri.etsi.org/ngsi-ld/default-context/temperature\">99").expect("valid q"));
        let stage1 = build_stage1_entity_ids(
            &HistoryBranch::live_recorded(),
            &compiled,
            &PageWindow::offset(20, 0),
            CountPolicy::Emit,
            &sensor_names(),
        )
        .expect("valid temporal query");
        let page = stage1.select.to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&page);
        assert!(branches.candidates.contains(r#"FROM "attribute_history" AS "rh" JOIN"#), "{page}");
        assert!(!branches.candidates.contains("value_number"), "{page}");
        assert!(branches.indexed.contains("value_number"), "{page}");
        let count = stage1.count.expect("count requested").to_string(PostgresQueryBuilder);
        assert!(!count.contains("candidates"), "{count}");
        assert_eq!(stage1.planning, StatementPlanning::BoundValues);
    }

    #[test]
    fn the_identifier_pattern_takes_precedence_over_the_scope_paths() {
        let mut compiled = id_pattern(Some("Sensor"), ".*:0123.*");
        compiled.entities.scope_q = Some(ScopeQuery::parse("/city/moved1").expect("valid scope query"));
        let sql = recorded_stage1(&compiled).select.to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(branches.candidates.contains(r#"FROM "entities" WHERE ("entities"."id" ~"#), "{sql}");
        assert!(!branches.candidates.contains("scope_history"), "{sql}");
        assert!(branches.indexed.contains(r#"FROM "scope_history" AS "rsh""#), "{sql}");
    }

    #[test]
    fn the_scope_paths_take_precedence_over_the_geometry_extents() {
        let mut compiled = near_query(4.5, "2000");
        compiled.entities.scope_q = Some(ScopeQuery::parse("/city/moved1").expect("valid scope query"));
        let sql = recorded_stage1(&compiled).select.to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(branches.candidates.contains(r#"FROM "scope_history" WHERE"#), "{sql}");
        assert!(!branches.candidates.contains("value_extent"), "{sql}");
        assert!(branches.indexed.contains(" _ST_DWithin("), "{sql}");
    }

    #[test]
    fn another_step_takes_precedence_over_the_window() {
        let mut compiled = sensors();
        compiled.window.timeproperty = TimeProp::Modified;
        compiled.window.interval = TimeInterval::After(instant("2026-10-18T03:00:00Z"));
        compiled.entities.scope_q = Some(ScopeQuery::parse("/city/moved1").expect("valid scope query"));
        let sql = recorded_stage1(&compiled).select.to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(branches.candidates.contains(r#"FROM "scope_history" WHERE"#), "{sql}");
        assert!(!branches.candidates.contains("attribute_history"), "{sql}");
        assert!(branches.indexed.contains(r#""ah"."modified_at" >= "#), "{sql}");
    }
}
