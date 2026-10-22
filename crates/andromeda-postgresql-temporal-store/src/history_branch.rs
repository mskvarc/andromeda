//! Deciding which relations of a branch a temporal read takes history from.
//!
//! A Snapshot branch has no queue. The live branch reads its queued entries
//! beside its recorded rows while the queue holds an intent, and its recorded
//! rows alone otherwise: a write acknowledged before the read began is either
//! applied or still queued when the probe runs, so the shortcut loses nothing
//! a client could see, and the union it saves costs about as much again as
//! the statement it extends. One probe serves a whole read: the read's scope
//! carries its answer to every statement.

use crate::{error::TemporalStoreError, repo::TemporalRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_query::history_source::HistoryBranch;
use andromeda_postgresql_schema::queue::TemporalAppendIntents;
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_temporal::port::read_scope::{ScopeReach, TemporalReadScope, UnrecordedHistory};
use sea_query::{Expr, ExprTrait, Order, Query, SelectStatement};

impl TemporalRepository {
    /// The scope a read of `target` that begins now runs in: a Snapshot
    /// branch has no queue, and the live branch holds unrecorded history
    /// while its queue holds an intent `reach` reads.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the queue probe fails.
    pub(crate) async fn scope_of<'a>(&self, target: &'a ContextTarget, reach: ScopeReach<'_>) -> Result<TemporalReadScope<'a>, TemporalStoreError> {
        let unrecorded = match target {
            ContextTarget::Snapshot(_) => UnrecordedHistory::Absent,
            ContextTarget::Live => {
                let first: Option<i64> = first_held_intent(reach).to_query_scalar().fetch_optional(&self.pool).await?;
                if first.is_some() {
                    UnrecordedHistory::Held
                } else {
                    UnrecordedHistory::Absent
                }
            }
        };
        Ok(TemporalReadScope::new(target, unrecorded))
    }
}

/// The relations a statement of the read `scope` takes history from: the
/// queued entries beside the recorded rows only on the live branch while it
/// held some.
pub(crate) fn branch_of(scope: &TemporalReadScope<'_>) -> HistoryBranch {
    match (schema_iden(scope.target()), scope.unrecorded()) {
        (Some(schema), _) => HistoryBranch::recorded_in(schema),
        (None, UnrecordedHistory::Held) => HistoryBranch::live(),
        (None, UnrecordedHistory::Absent) => HistoryBranch::live_recorded(),
    }
}

/// The first intent the live branch's queue holds that `reach` reads: any
/// intent, or one of the Entity.
///
/// The probe walks an index in key order (the primary key, or
/// `idx_temporal_append_intents_entity` for one Entity) rather than letting
/// an `EXISTS` scan the heap: a drain leaves the queue's pages full of dead
/// rows until vacuum, which a heap scan reads on every probe and an index
/// scan marks dead once and skips after.
fn first_held_intent(reach: ScopeReach<'_>) -> SelectStatement {
    let mut intent = Query::select();
    intent.column(TemporalAppendIntents::IntentKey).from(TemporalAppendIntents::Table);
    match reach {
        ScopeReach::Every => {}
        ScopeReach::Entity(entity_id) => {
            intent.and_where(Expr::col(TemporalAppendIntents::EntityId).eq(entity_id.as_str()));
        }
    }
    intent.order_by(TemporalAppendIntents::IntentKey, Order::Asc).limit(1);
    intent
}

#[cfg(test)]
mod tests {
    use crate::history_branch::first_held_intent;
    use andromeda_temporal::port::read_scope::ScopeReach;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_probe_reads_the_first_queued_intent_in_key_order() {
        pretty_assertions::assert_eq!(
            first_held_intent(ScopeReach::Every).to_string(PostgresQueryBuilder),
            r#"SELECT "intent_key" FROM "temporal_append_intents" ORDER BY "intent_key" ASC LIMIT 1"#
        );
    }

    /// A read of one Entity reads no other Entity's queued entries, so only
    /// that Entity's intents decide whether it reads the queue.
    #[test]
    fn a_read_of_one_entity_probes_for_its_intents_alone() {
        let entity_id = "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI");
        pretty_assertions::assert_eq!(
            first_held_intent(ScopeReach::Entity(&entity_id)).to_string(PostgresQueryBuilder),
            r#"SELECT "intent_key" FROM "temporal_append_intents" WHERE "entity_id" = 'urn:ngsi-ld:Vehicle:1' ORDER BY "intent_key" ASC LIMIT 1"#
        );
    }
}
