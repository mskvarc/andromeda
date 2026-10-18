//! The candidate step a stage 1 statement takes: one bounded index read that
//! names the few Evolutions a filter can keep, so the page and the count are
//! cut from the walk narrowed to them rather than from every Evolution the
//! walk yields.
//!
//! A statement takes one step at most. Every filter the step does not answer
//! is tested on the narrowed walk, and a request no step applies to walks as
//! before. A `q` term gets no step: history holds no index of its values that
//! a numeric write could afford to keep.

use crate::stage1::{entity_conditions::PatternTest, pattern_candidates::pattern_candidates};
use andromeda_ngsi_ld_query::{entity_selection::ConsideredEntities, pattern::Pattern, temporal::compiled::CompiledTemporalQuery};
use andromeda_postgresql_query::{
    history_source::HistoryBranch,
    id_pattern_trigrams::{ID_PATTERN_CANDIDATE_BOUND, ID_PATTERN_SHORT_WALK, trigram_pattern},
    one_time_gate::COUNT_CANDIDATE_BOUND,
};
use sea_query::SelectStatement;

/// The candidate step a stage 1 statement takes.
#[derive(Debug, Clone, Copy)]
pub(super) enum CandidateStep<'q> {
    /// The lone selector's `idPattern` (clause 5.7.2), when it leaves the
    /// identifier's start open: the catalogue's trigram index yields the
    /// identifiers it matches, each rechecked against it.
    IdPattern(&'q Pattern),
}

impl<'q> CandidateStep<'q> {
    /// The step stage 1 takes for `compiled`, `None` when the walk answers
    /// alone.
    ///
    /// The identifier pattern applies when one selector carries it and no
    /// ordering is requested: several selectors pair each pattern with its own
    /// type, which one read of the trigrams cannot, and a requested ordering
    /// cuts the page in an order the candidates are not read in.
    pub(super) fn chosen(compiled: &'q CompiledTemporalQuery) -> Option<CandidateStep<'q>> {
        match compiled.entities.considered {
            ConsideredEntities::Selected => {}
            ConsideredEntities::NoEntity => return None,
        }
        if compiled.ordering.is_some() {
            return None;
        }
        let [selector] = compiled.entities.selectors.as_slice() else {
            return None;
        };
        trigram_pattern(selector.identifier_axis()).map(CandidateStep::IdPattern)
    }

    /// How many Evolutions the short walk visits before the candidates are
    /// read.
    pub(super) const fn short_walk(self) -> u32 {
        match self {
            CandidateStep::IdPattern(_) => ID_PATTERN_SHORT_WALK,
        }
    }

    /// The most candidates a page reads before it walks instead.
    pub(super) const fn bound(self) -> u64 {
        match self {
            CandidateStep::IdPattern(_) => ID_PATTERN_CANDIDATE_BOUND,
        }
    }

    /// The most candidates a count reads before it walks instead.
    pub(super) const fn count_bound(self) -> u64 {
        match self {
            CandidateStep::IdPattern(_) => COUNT_CANDIDATE_BOUND,
        }
    }

    /// Whether the narrowed walk still tests the selector's pattern.
    pub(super) const fn pattern_test(self) -> PatternTest {
        match self {
            CandidateStep::IdPattern(_) => PatternTest::Answered,
        }
    }

    /// The candidates of `branch`, unbounded: surrogate keys over recorded
    /// rows alone, identifiers where the branch reads its queue.
    pub(super) fn candidates(self, branch: &HistoryBranch) -> SelectStatement {
        match self {
            CandidateStep::IdPattern(pattern) => pattern_candidates(branch, pattern),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::stage1::{
        build_stage1_entity_ids,
        query_fixture::{sensor_names, sensors},
    };
    use andromeda_ngsi_ld_query::{count_policy::CountPolicy, filter::parser, page_window::PageWindow};
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
}
