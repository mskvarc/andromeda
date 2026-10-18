//! The Entities a branch reading its queue adds to a candidate step's
//! candidates.
//!
//! No candidate index holds a queued entry, so where the branch reads its
//! queue the candidates are identifiers: the recorded candidates' and every
//! Entity a queued intent names, which a step may narrow by what the
//! identifier alone decides. The walk is then narrowed by identifier, and a
//! backlog that passes the step's bound walks.

use andromeda_postgresql_query::history_source::HistoryBranch;
use andromeda_postgresql_schema::queue::TemporalAppendIntents;
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{Query, SelectStatement};

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
