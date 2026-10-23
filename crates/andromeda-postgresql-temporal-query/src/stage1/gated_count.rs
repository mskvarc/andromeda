//! The stage 1 count a candidate step cuts (clause 6.3.13).
//!
//! The count is exact, so it needs every match. Read from the step's index it
//! costs the candidates; walked, it costs every Evolution of the walk. The
//! statement reads at most `bound` plus one candidate behind a one-time
//! gate: when they number at most the bound they are every candidate, and the
//! keys of the walk narrowed to them are counted; when more qualify, the keys
//! of the whole walk are.
//!
//! One count reads the keys either gate admits, each Evolution looked up by
//! its key and tested for every filter, those the step answers included. A
//! count per gate would be planned in full on every execution, and the
//! filters it repeats are each a probe over the history of the tested
//! streams. The lookup is fenced behind `OFFSET 0`: merged into the statement
//! it becomes a join PostgreSQL plans over the whole catalogue, whichever gate
//! is open.

use crate::stage1::{
    candidate_step::CandidateStep,
    counted_ids,
    entity_conditions::{PatternTest, build_entity_conditions_for_temporal},
    evolution_walk::{Among, evolution_rows, walked_key},
    window_match::temporal_match_subquery,
};
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_query::{
    dictionary::query_names::QueryNames,
    error::RenderError,
    history_source::{BesideComparison, HistoryBranch, entity_rows::entity_rows},
    one_time_gate::{Candidates, beyond, bounded, materialized, within},
};
use andromeda_postgresql_schema::current::{E, Entities};
use sea_query::{Asterisk, Expr, ExprTrait, Iden, IntoIden, JoinType, Query, SelectStatement, SimpleExpr, TableRef, UnionType, WithClause};

/// Alias `k`: the key of one Evolution the count looks up.
#[derive(Iden, Clone, Copy)]
#[iden = "k"]
struct CountedKey;

/// The count of stage 1 that `step` cuts from at most `bound` candidates.
///
/// # Errors
///
/// Fails as rendering the plain count fails.
pub(super) fn gated_count(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    names: &QueryNames,
    step: &CandidateStep<'_>,
    bound: u64,
) -> Result<SelectStatement, RenderError> {
    let walked = evolution_rows(branch, compiled, names, Among::Every);
    let narrowed = evolution_rows(branch, compiled, names, Among::Candidates);
    let conditions = build_entity_conditions_for_temporal(branch, compiled, names, walked.type_test, PatternTest::Tested)?;
    let key = walked_key(branch);
    let mut keys = keys_of(narrowed.counted, key);
    keys.and_where(within(Candidates, bound));
    let mut walked_keys = keys_of(walked.counted, key);
    walked_keys.and_where(beyond(Candidates, bound));
    keys.union(UnionType::All, walked_keys);
    let window_match = temporal_match_subquery(branch, compiled, names, BesideComparison::Key);
    let mut count = counted_ids(looked_up(branch, keys, key), conditions, window_match);
    count.with_cte(
        WithClause::new()
            .cte(materialized(bounded(step.candidates(branch), bound), Candidates))
            .to_owned(),
    );
    Ok(count)
}

/// `SELECT e.<key> FROM <evolutions>`.
fn keys_of(evolutions: TableRef, key: Entities) -> SelectStatement {
    let mut keys = Query::select();
    keys.column((E, key)).from(evolutions);
    keys
}

/// The Evolutions of `branch` whose `key` `keys` yields, under `e`, each
/// looked up in the catalogue on its own behind `OFFSET 0`.
fn looked_up(branch: &HistoryBranch, keys: SelectStatement, key: Entities) -> TableRef {
    let mut lookup = Query::select();
    lookup
        .column(Asterisk)
        .from(entity_rows(branch, E))
        .and_where(Expr::col((E, key)).equals((CountedKey, key)))
        .offset(0);
    let mut evolutions = Query::select();
    evolutions
        .column((E, Asterisk))
        .from_subquery(keys, CountedKey)
        .join_lateral(JoinType::Join, lookup, E, SimpleExpr::from(true));
    TableRef::SubQuery(Box::new(evolutions), E.into_iden())
}
