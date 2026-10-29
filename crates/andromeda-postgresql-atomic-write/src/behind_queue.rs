//! A live synchronous commit's history, recorded inline for its Entities with
//! no queued history and queued behind the history of the others.
//!
//! Clauses 4.5.7, 4.5.8 and 4.8 read an Entity's history in the order its
//! writes committed. An Entity whose earlier respond-async history still waits
//! in the queue cannot have this commit's history recorded ahead of it: a
//! deletion would be judged against an evolution not yet written, and a
//! modification instant would land before an earlier one. Its intents join
//! the queue behind the earlier ones, and the drain applies them in order.
//!
//! The probe runs after the commit has written its Entity rows, so the row
//! locks are held: every earlier writer of each Entity has committed its
//! intents, and an intent a drain is applying but has not committed is still
//! seen queued. Each intent is numbered from its own base ordinal, so splitting
//! the commit's intents by Entity leaves every history row's identity as it
//! was. A Temporal Evolution the commit's create established stands only for
//! an Entity recorded inline; one queued behind gets it back
//! ([`evolution_establishment`]).

use crate::{
    attempt_failure::AttemptFailure,
    commit_refusal::temporal,
    consequences::ConsequenceWrite,
    deferred_history,
    evolution_establishment,
    history,
    intent_store::PostgresTemporalIntentStore,
};
use andromeda_context_commit::write_record::WriteRecord;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_temporal_store::recorder::TemporalRecorder;
use andromeda_temporal::append_intent::TemporalAppendIntent;
use sqlx::{Postgres, Transaction};
use std::collections::HashSet;

/// Record the live-branch intents of the commit `record` inline for every
/// Entity with no queued history, and queue those of the others, reporting
/// whether anything was queued. `established` names the Entities whose
/// Temporal Evolution the commit's create wrote.
pub(crate) async fn apply(
    tx: &mut Transaction<'_, Postgres>,
    recorder: &TemporalRecorder,
    record: &WriteRecord,
    intents: Vec<TemporalAppendIntent>,
    established: Vec<Identifier>,
) -> Result<ConsequenceWrite, AttemptFailure> {
    let entity_ids: Vec<&Identifier> = intents.iter().map(|intent| &intent.entity_id).collect();
    let queued = PostgresTemporalIntentStore::queued_entities_in_tx(tx, &entity_ids)
        .await
        .map_err(|error| AttemptFailure::Failed(temporal(error)))?;
    let EntitySplit { inline, behind } = split_behind(intents, &queued);
    let established = evolution_establishment::confirm(tx, &ContextTarget::Live, &inline, established).await?;
    history::apply(recorder, tx, &ContextTarget::Live, record, &inline, &established).await?;
    Ok(deferred_history::apply(tx, recorder, record, &behind).await?)
}

/// One commit's intents split by whether their Entity has queued history,
/// each side in commit order.
struct EntitySplit {
    /// The intents of the Entities with no queued history.
    inline: Vec<TemporalAppendIntent>,
    /// The intents of the Entities with queued history.
    behind: Vec<TemporalAppendIntent>,
}

/// Split `intents` by whether their Entity is in `queued`, so every intent of
/// one Entity is on one side.
fn split_behind(intents: Vec<TemporalAppendIntent>, queued: &HashSet<Identifier>) -> EntitySplit {
    let (behind, inline) = intents.into_iter().partition(|intent| queued.contains(&intent.entity_id));
    EntitySplit { inline, behind }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::behind_queue::{EntitySplit, split_behind};
    use andromeda_context_commit::{write_ordinal::WriteOrdinal, write_record::WriteRecord};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_temporal::append_intent::TemporalAppendIntent;
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use std::collections::HashSet;

    fn vehicle(index: u8) -> Identifier {
        format!("urn:ngsi-ld:Vehicle:{index}").parse().expect("a valid identifier")
    }

    /// The intent of `vehicle(index)` numbered from `base`.
    fn intent(index: u8, base: u32) -> TemporalAppendIntent {
        TemporalAppendIntent {
            record: WriteRecord::mint(Utc.timestamp_opt(1_700_000_000, 0).single().expect("a representable instant")),
            base_ordinal: WriteOrdinal::at(base),
            entity_id: vehicle(index),
            entity_types: Vec::new(),
            entries: Vec::new(),
        }
    }

    /// The Entity and base ordinal of each of `intents`, in order.
    fn named(intents: Vec<TemporalAppendIntent>) -> Vec<(Identifier, u32)> {
        intents.into_iter().map(|intent| (intent.entity_id, intent.base_ordinal.get())).collect()
    }

    #[test]
    fn an_entity_with_queued_history_sends_every_intent_behind_it_in_order() {
        let intents = vec![intent(1, 0), intent(2, 3), intent(1, 5)];
        let EntitySplit { inline, behind } = split_behind(intents, &HashSet::from([vehicle(1)]));
        assert_eq!(named(inline), vec![(vehicle(2), 3)]);
        assert_eq!(named(behind), vec![(vehicle(1), 0), (vehicle(1), 5)]);
    }

    #[test]
    fn an_entity_without_queued_history_is_recorded_inline() {
        let intents = vec![intent(1, 0), intent(2, 3)];
        let EntitySplit { inline, behind } = split_behind(intents, &HashSet::from([vehicle(9)]));
        assert_eq!(named(inline), vec![(vehicle(1), 0), (vehicle(2), 3)]);
        assert_eq!(named(behind), Vec::new());
    }

    #[test]
    fn no_queued_entity_leaves_every_intent_inline() {
        let intents = vec![intent(1, 0), intent(1, 2)];
        let EntitySplit { inline, behind } = split_behind(intents, &HashSet::new());
        assert_eq!(named(inline), vec![(vehicle(1), 0), (vehicle(1), 2)]);
        assert_eq!(named(behind), Vec::new());
    }
}
