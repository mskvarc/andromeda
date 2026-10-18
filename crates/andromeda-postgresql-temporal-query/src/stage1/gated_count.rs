//! The stage 1 count a candidate step cuts (clause 6.3.13).
//!
//! The count is exact, so it needs every match. Read from the step's index it
//! costs the candidates, each tested for the filters the step does not answer;
//! walked, it costs every Evolution of the walk, each tested for every filter.
//! The statement reads at most the step's count bound plus one candidate
//! behind a one-time gate: when they number at most the bound they are every
//! candidate, and the walk narrowed to them is counted; when more qualify, the
//! walk is.

use crate::stage1::{
    candidate_step::CandidateStep,
    counted_ids,
    entity_conditions::{PatternTest, build_entity_conditions_for_temporal},
    evolution_walk::{Among, evolution_rows},
    window_match::temporal_match_subquery,
};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_query::{
    dictionary::query_names::QueryNames,
    error::RenderError,
    history_source::{BesideComparison, HistoryBranch},
    one_time_gate::{Candidates, bounded, materialized, within},
};
use sea_query::{Expr, Query, SelectStatement, WithClause};

/// The count of stage 1 that `step` cuts.
///
/// # Errors
///
/// Fails as rendering the plain count fails.
pub(super) fn gated_count(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    names: &QueryNames,
    step: &CandidateStep<'_>,
) -> Result<SelectStatement, RenderError> {
    let window_match = || temporal_match_subquery(branch, compiled, names, BesideComparison::Key);
    let walked = evolution_rows(branch, compiled, names, Among::Every);
    let whole = build_entity_conditions_for_temporal(branch, compiled, names, walked.type_test, PatternTest::Tested)?;
    let narrowed = evolution_rows(branch, compiled, names, Among::Candidates);
    let residual = build_entity_conditions_for_temporal(branch, compiled, names, narrowed.type_test, step.pattern_test())?;
    let indexed = counted_ids(narrowed.counted, residual, window_match());
    let walk = counted_ids(walked.counted, whole, window_match());
    let mut count = Query::select();
    count.expr(Expr::case(within(Candidates, step.count_bound()), indexed).finally(walk)).with_cte(
        WithClause::new()
            .cte(materialized(bounded(step.candidates(branch), step.count_bound()), Candidates))
            .to_owned(),
    );
    Ok(count.take())
}
