//! The one numbering of a commit's consequences.

use crate::{
    attempt_failure::AttemptFailure,
    consequences::notification_events::{self, EventGroup},
    history_interpretation::interpret,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    commit_invariant::CommitInvariant,
    error::CommitError,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_temporal::{append_intent::TemporalAppendIntent, core_write_history::CoreWriteHistory};

/// Every consequence one commit's landed writes owe, numbered.
///
/// One commit numbers its consequences with one running counter: the history
/// intents draw first, then the notification events, and the side-effect job
/// stores the first ordinal left, which its replay resumes from.
pub(crate) struct NumberedConsequences<'a> {
    /// The history intents, each at its base ordinal.
    pub(crate) intents: Vec<TemporalAppendIntent>,
    /// The notification events, one per group.
    pub(crate) events: Vec<EventGroup<'a>>,
    /// The ordinal the first event draws.
    pub(crate) events_base: WriteOrdinal,
    /// The first ordinal the events left free, the job's base.
    pub(crate) job_base: WriteOrdinal,
}

/// Number every consequence `landed` owes from `base`.
///
/// # Errors
///
/// Returns [`AttemptFailure`] when a write's history does not interpret, or
/// the consequences outgrow the ordinal range.
pub(crate) fn number_consequences<'a>(
    landed: &[&'a AtomicWrite],
    record: &WriteRecord,
    base: WriteOrdinal,
    core_write_history: CoreWriteHistory,
) -> Result<NumberedConsequences<'a>, AttemptFailure> {
    let (intents, events_base) = interpret(landed, record, base, core_write_history)?;
    let events = notification_events::group(landed);
    let advance = u32::try_from(events.len()).map_err(|error| CommitError::from(CommitInvariant::ConsequenceCount(error)))?;
    let job_base = events_base
        .advanced_by(advance)
        .map_err(|error| CommitError::from(CommitInvariant::OrdinalOverflow(error)))?;
    Ok(NumberedConsequences {
        intents,
        events,
        events_base,
        job_base,
    })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::consequences::numbering::{NumberedConsequences, number_consequences};
    use andromeda_context_commit::{
        atomic_write::AtomicWrite,
        batch_side_effect_job::BatchSideEffectJob,
        change_event::ChangeEvent,
        entity_mutation::EntityMutation,
        history_record::HistoryRecord,
        subscription_key::SubscriptionKey,
        subscription_match::SubscriptionMatch,
        write_id::WriteId,
        write_ordinal::WriteOrdinal,
        write_record::WriteRecord,
    };
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::{
        entity_change::{EntityChange, EntityLifecycle},
        notification::ChangeOrigin,
    };
    use andromeda_temporal::core_write_history::CoreWriteHistory;
    use chrono::Utc;
    use serde_json::json;
    use std::sync::Arc;

    /// A Vehicle `id` observed with three Properties: one intent of three
    /// entries.
    fn observed(id: &str) -> Arc<EntityState> {
        let property = |value: i64| json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]}]);
        let document = json!([{
            "@id": id,
            "@type": ["https://example.org/Vehicle"],
            "https://example.org/a": property(1),
            "https://example.org/b": property(2),
            "https://example.org/c": property(3)
        }]);
        Arc::new(EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity"))
    }

    /// A respond-async write of `id` that observes three Properties, matched
    /// one subscription and owes one change in its job.
    fn enqueued_write(id: &str, subscription: i64) -> AtomicWrite {
        let entity_id: Identifier = id.parse().expect("a valid entity URI");
        let change = EntityChange::new(EntityLifecycle::Modified, Vec::new());
        AtomicWrite::builder()
            .entity_id(Arc::new(entity_id.clone())) // clone: the write and its consequences each own the identifier
            .entity(EntityMutation::None)
            .history(vec![HistoryRecord::Observations(observed(id))])
            .subscription_matches(vec![
                SubscriptionMatch::builder()
                    .subscription_key(SubscriptionKey::try_from(subscription).expect("a positive key"))
                    .entity_id(entity_id.clone()) // clone: the match and the change each own the identifier
                    .change(change.clone()) // clone: the match and the change each own the change
                    .build(),
            ])
            .side_effects(Some(BatchSideEffectJob {
                change_events: vec![
                    ChangeEvent::builder()
                        .entity_id(entity_id)
                        .change(change)
                        .occurred_at(Utc::now())
                        .origin(ChangeOrigin::Local)
                        .build(),
                ],
            }))
            .build()
    }

    /// One commit numbers its history entries, then its notification events,
    /// and its job resumes after both, so no two consequences of one write
    /// identity share an ordinal.
    #[test]
    fn intents_events_and_the_job_draw_ordinals_in_that_order() {
        let (first, second) = (enqueued_write("urn:ngsi-ld:Vehicle:1", 1), enqueued_write("urn:ngsi-ld:Vehicle:2", 2));
        let record = WriteRecord::restored(WriteId::mint(), Utc::now());

        let NumberedConsequences {
            intents,
            events,
            events_base,
            job_base,
        } = number_consequences(&[&first, &second], &record, WriteOrdinal::at(0), CoreWriteHistory::Recorded).expect("the consequences number");

        let intent_ordinals: Vec<(WriteOrdinal, usize)> = intents.iter().map(|intent| (intent.base_ordinal, intent.entries.len())).collect();
        pretty_assertions::assert_eq!(intent_ordinals, vec![(WriteOrdinal::at(0), 3), (WriteOrdinal::at(3), 3)]);
        pretty_assertions::assert_eq!(events.len(), 2);
        pretty_assertions::assert_eq!(events_base, WriteOrdinal::at(6));
        pretty_assertions::assert_eq!(job_base, WriteOrdinal::at(8));
    }
}
