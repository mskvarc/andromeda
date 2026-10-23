//! The Entities a branch reading its queue adds to a candidate step's
//! candidates.
//!
//! No candidate index holds a queued entry, so where the branch reads its
//! queue the candidates are identifiers: the recorded candidates' and every
//! Entity a queued intent names, which a step may narrow by what the
//! identifier alone decides. The walk is then narrowed by identifier, and a
//! backlog that passes the step's bound walks.

use crate::alias::Rs;
use andromeda_postgresql_query::history_source::{HistoryBranch, PendingEntries};
use andromeda_postgresql_schema::{current::Entities, queue::TemporalAppendIntents, temporal::TemporalAttributeStreams};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{Expr, ExprTrait, Iden, JoinType, Query, SelectStatement};

/// Alias `ri`: the identity row of a candidate stream's Entity.
#[derive(Iden, Clone, Copy)]
#[iden = "ri"]
struct CandidateIdentity;

/// Project the Evolution the recorded stream under `rs` belongs to in
/// `candidates`, as the candidates of `branch` name it: its surrogate key over
/// recorded rows alone; where the branch reads its queue, its identifier, read
/// from the stream's identity row, beside the identifiers of the queued
/// Entities.
pub(super) fn stream_holder(candidates: &mut SelectStatement, branch: &HistoryBranch) {
    match branch.pending() {
        PendingEntries::Absent => {
            candidates.column((Rs, TemporalAttributeStreams::EntityKey));
        }
        PendingEntries::Unioned => {
            candidates.column((CandidateIdentity, Entities::Id)).join_as(
                JoinType::Join,
                qualified(branch.schema(), Entities::Table),
                CandidateIdentity,
                Expr::col((CandidateIdentity, Entities::EntityKey)).equals((Rs, TemporalAttributeStreams::EntityKey)),
            );
        }
    }
}

/// `SELECT "entity_id" FROM "temporal_append_intents"` of `branch`: an Entity
/// once for each of its queued intents.
pub(super) fn queued_candidates(branch: &HistoryBranch) -> SelectStatement {
    Query::select()
        .column(TemporalAppendIntents::EntityId)
        .from(qualified(branch.schema(), TemporalAppendIntents::Table))
        .take()
}

#[cfg(test)]
mod tests {
    use crate::stage1::queued_candidates::queued_candidates;
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn every_entity_a_queued_intent_names_is_a_candidate() {
        assert_eq!(
            queued_candidates(&HistoryBranch::live()).to_string(PostgresQueryBuilder),
            r#"SELECT "entity_id" FROM "temporal_append_intents""#
        );
    }
}
