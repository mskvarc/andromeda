//! The candidate step a stage 1 statement takes: one bounded index read that
//! names the few Evolutions a filter can keep, so the page and the count are
//! cut from the walk narrowed to them rather than from every Evolution the
//! walk yields.
//!
//! A statement takes one step at most. Every filter the step does not answer
//! is tested on the narrowed walk, and a request no step applies to walks as
//! before. A `q` term gets no step: history holds no index of its values that
//! a numeric write could afford to keep.

use crate::stage1::{
    entity_conditions::PatternTest,
    extent_candidates::{EXTENT_CANDIDATE_BOUND, EXTENT_SHORT_WALK, ExtentRead},
    pattern_candidates::pattern_candidates,
    scope_candidates::{SCOPE_CANDIDATE_BOUND, SCOPE_SHORT_WALK, scope_candidates},
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
}

impl<'q> CandidateStep<'q> {
    /// The step stage 1 takes for `compiled`, `None` when the walk answers
    /// alone.
    ///
    /// The cheapest bounded read that applies is taken: the identifier
    /// pattern, then the Scope paths, then the geometry extents. A requested
    /// ordering cuts the page in an order no candidates are read in, so it
    /// keeps the walk. The identifier pattern applies when one selector
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
        Ok(ExtentRead::of(compiled, names)?.map(CandidateStep::GeometryExtents))
    }

    /// How many Evolutions the short walk visits before the candidates are
    /// read.
    pub(super) const fn short_walk(&self) -> u32 {
        match self {
            CandidateStep::IdPattern(_) => ID_PATTERN_SHORT_WALK,
            CandidateStep::ScopePaths(..) => SCOPE_SHORT_WALK,
            CandidateStep::GeometryExtents(_) => EXTENT_SHORT_WALK,
        }
    }

    /// The most candidates a page reads before it walks instead.
    pub(super) const fn bound(&self) -> u64 {
        match self {
            CandidateStep::IdPattern(_) => ID_PATTERN_CANDIDATE_BOUND,
            CandidateStep::ScopePaths(..) => SCOPE_CANDIDATE_BOUND,
            CandidateStep::GeometryExtents(_) => EXTENT_CANDIDATE_BOUND,
        }
    }

    /// The most candidates a count reads before it walks instead.
    pub(super) const fn count_bound(&self) -> u64 {
        match self {
            CandidateStep::IdPattern(_) | CandidateStep::ScopePaths(..) | CandidateStep::GeometryExtents(_) => COUNT_CANDIDATE_BOUND,
        }
    }

    /// Whether the narrowed walk still tests the selector's pattern.
    pub(super) const fn pattern_test(&self) -> PatternTest {
        match self {
            CandidateStep::IdPattern(_) => PatternTest::Answered,
            CandidateStep::ScopePaths(..) | CandidateStep::GeometryExtents(_) => PatternTest::Tested,
        }
    }

    /// The candidates of `branch`, unbounded: surrogate keys over recorded
    /// rows alone, identifiers where the branch reads its queue.
    pub(super) fn candidates(&self, branch: &HistoryBranch) -> SelectStatement {
        match self {
            CandidateStep::IdPattern(pattern) => pattern_candidates(branch, pattern),
            CandidateStep::ScopePaths(scope_q, contributed) => scope_candidates(branch, scope_q, contributed),
            CandidateStep::GeometryExtents(extents) => extents.candidates(branch),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::stage1::{
        build_stage1_entity_ids,
        query_fixture::{PageBranches, id_pattern, near_query, recorded_stage1, sensor_names, sensors},
    };
    use andromeda_ngsi_ld_query::{count_policy::CountPolicy, filter::parser, page_window::PageWindow, scope::query::ScopeQuery};
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn a_q_filter_keeps_the_walk() {
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
        assert!(!page.contains("candidates"), "{page}");
        let count = stage1.count.expect("count requested").to_string(PostgresQueryBuilder);
        assert!(!count.contains("candidates"), "{count}");
        assert_eq!(stage1.planning, StatementPlanning::Cached);
    }

    #[test]
    fn the_identifier_pattern_takes_precedence_over_the_scope_paths() {
        let mut compiled = id_pattern(Some("Sensor"), ".*:0123.*");
        compiled.entities.scope_q = Some(ScopeQuery::parse("/city/moved1").expect("valid scope query"));
        let sql = recorded_stage1(&compiled).select.to_string(PostgresQueryBuilder);
        let branches = PageBranches::of(&sql);
        assert!(
            branches.candidates.contains(r#"FROM "temporal_entities" WHERE "temporal_entities"."id" ~"#),
            "{sql}"
        );
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
        assert!(branches.indexed.contains("ST_DWithin("), "{sql}");
    }
}
