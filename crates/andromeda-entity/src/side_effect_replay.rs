//! The writes a drained side-effect job replays.

use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, subscription_match::SubscriptionMatch};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::{collections::HashMap, sync::Arc};

/// The writes one job replays: one per entity, carrying the durable matches
/// its owed changes are due.
///
/// A commit names each entity once, so the matches of one entity join one
/// write. No write changes the entity row or records history: the history was
/// queued as intents by the commit that enqueued the job. The writes follow
/// the order the matches first name their entity, and every write keeps its
/// matches in their order, which is what numbers the replay's rows the same
/// way on every attempt.
pub(crate) fn replay_writes(owed: Vec<SubscriptionMatch>) -> Vec<AtomicWrite> {
    let mut writes: Vec<AtomicWrite> = Vec::new();
    let mut positions: HashMap<Arc<Identifier>, usize> = HashMap::new();
    for matched in owed {
        if let Some(write) = positions.get(&matched.entity_id).and_then(|position| writes.get_mut(*position)) {
            write.subscription_matches.push(matched);
        } else {
            let identifier = Arc::new(matched.entity_id.clone()); // clone: the write owns the identifier the match also keeps.
            positions.insert(Arc::clone(&identifier), writes.len());
            writes.push(
                AtomicWrite::builder()
                    .entity_id(identifier)
                    .entity(EntityMutation::None)
                    .subscription_matches(vec![matched])
                    .build(),
            );
        }
    }
    writes
}

#[cfg(test)]
mod tests {
    use crate::{building_subscription_fixture::subscription_key, side_effect_replay::replay_writes};
    use andromeda_context_commit::subscription_match::SubscriptionMatch;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};

    fn matched(id: &str) -> SubscriptionMatch {
        SubscriptionMatch::builder()
            .subscription_key(subscription_key())
            .entity_id(id.parse::<Identifier>().expect("a valid URI"))
            .change(EntityChange::new(EntityLifecycle::Modified, Vec::new()))
            .build()
    }

    /// The commit that enqueued the job already queued its history, so a
    /// replay of three owed changes of two Entities is two writes that record
    /// their matches and nothing else.
    #[test]
    fn a_replay_commits_notification_consequences_only() {
        let (first, second) = ("urn:ngsi-ld:Building:1", "urn:ngsi-ld:Building:2");
        let writes = replay_writes(vec![matched(first), matched(second), matched(first)]);

        let shape: Vec<(String, usize)> = writes
            .iter()
            .map(|write| (write.entity_id.as_str().to_owned(), write.subscription_matches.len()))
            .collect();
        pretty_assertions::assert_eq!(shape, vec![(first.to_owned(), 2), (second.to_owned(), 1)]);
        assert!(writes.iter().all(|write| write.entity.is_none()), "a replay never changes the entity row");
        assert!(writes.iter().all(|write| write.history.is_empty()), "a replay records no history");
        assert!(writes.iter().all(|write| write.side_effects.is_none()), "a replay queues no job");
    }
}
